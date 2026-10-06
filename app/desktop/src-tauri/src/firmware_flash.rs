use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const BOOTLOADER_SHA: &str = "be3abea605a6be7f04c2d0f4011bd90688f799a834a164cdc6a29b16c3324287";
pub const PARTITION_SHA: &str = "7c541b70dcac8f920c2d11589f06745e1b033fa9b95b8343de2748bb8312a278";
pub const FIRMWARE_SHA: &str = "61f08d4caf8d00eda9862a250f99abc2a4f2b58be8b6a6917a9f0c3a71bc633a";

#[derive(Clone, Serialize)]
pub struct FirmwareInfo {
    pub latest_version: &'static str,
    pub name: &'static str,
    pub sha256: String,
    pub images: Vec<RestoreImage>,
    pub available: bool,
    pub recovery_configured: bool,
    pub recovery_ssid: Option<String>,
    pub recovery_host: Option<String>,
}

#[derive(Clone, Copy, Serialize)]
pub struct RestoreImage {
    pub name: &'static str,
    pub file: &'static str,
    pub address: u32,
    pub sha256: &'static str,
    #[serde(skip)]
    max_size: usize,
}

const IMAGES: [RestoreImage; 3] = [
    RestoreImage {
        name: "引导程序",
        file: "current-bootloader.bin",
        address: 0,
        sha256: BOOTLOADER_SHA,
        max_size: 0x8000,
    },
    RestoreImage {
        name: "分区表",
        file: "current-partition-table.bin",
        address: 0x8000,
        sha256: PARTITION_SHA,
        max_size: 0x1000,
    },
    RestoreImage {
        name: "VoxQueue 应用",
        file: "current-firmware.bin",
        address: 0x10000,
        sha256: FIRMWARE_SHA,
        max_size: 0x300000,
    },
];

fn restore_sha() -> String {
    // Fingerprint the entire reviewed restore plan, including write addresses.
    let mut digest = Sha256::new();
    for image in IMAGES {
        digest.update(format!("{:08x}:{}\n", image.address, image.sha256));
    }
    format!("{:x}", digest.finalize())
}

pub fn info(resources: &Path) -> FirmwareInfo {
    FirmwareInfo {
        recovery_configured: recovery_configured(),
        recovery_ssid: recovery_network().map(|network|network.0),
        recovery_host: recovery_network().map(|network|network.1),
        latest_version: easy_codex_host::lan_voice::CURRENT_FIRMWARE_VERSION,
        name: "2026.10.06 · 版本上报与播报试听 · VoxQueue 完整恢复包",
        sha256: restore_sha(),
        images: IMAGES.to_vec(),
        available: cfg!(windows)
            && IMAGES.iter().all(|i| resources.join(i.file).is_file())
            && resources.join("esptool/esptool.exe").is_file(),
    }
}

fn write_arguments(resources: &Path, port: &str) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = [
        "--chip",
        "esp32s3",
        "--port",
        port,
        "--baud",
        "460800",
        "--before",
        "usb_reset",
        "--after",
        "hard_reset",
        "--connect-attempts",
        "1",
        "write_flash",
        "--flash_mode",
        "dio",
        "--flash_freq",
        "80m",
        "--flash_size",
        "16MB",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    for image in IMAGES {
        args.push(format!("0x{:x}", image.address).into());
        args.push(resources.join(image.file).into_os_string());
    }
    args
}

#[derive(Clone, Default, Serialize)]
pub struct FlashSnapshot {
    pub phase: String,
    pub progress: Option<u8>,
    pub message: String,
    pub log: Vec<String>,
}

#[derive(Default)]
struct State {
    snapshot: FlashSnapshot,
    cancelled: bool,
    verified_images: usize,
}

#[derive(Clone, Default)]
pub struct FirmwareFlasher(Arc<Mutex<State>>);

impl FirmwareFlasher {
    pub fn snapshot(&self) -> FlashSnapshot {
        self.0.lock().unwrap().snapshot.clone()
    }

    pub fn busy(&self) -> bool {
        matches!(self.snapshot().phase.as_str(), "waiting" | "flashing" | "restoring" | "connecting")
    }

    pub fn cancel_wait(&self) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        if state.snapshot.phase != "waiting" {
            return Err("只能取消等待，写入固件时请保持连接".into());
        }
        state.cancelled = true;
        Ok(())
    }

    pub fn start(&self, resources: PathBuf, reviewed_sha: String) -> Result<FlashSnapshot, String> {
        if !cfg!(windows) {
            return Err("当前烧录功能支持 Windows".into());
        }
        validate_restore(&resources, &reviewed_sha)?;
        #[cfg(all(windows, not(test)))]
        if !recovery_configured() { return Err("请先保存配网恢复配置，再开始完整恢复".into()); }
        if !resources.join("esptool/esptool.exe").is_file() {
            return Err("安装包缺少烧录工具，请重新安装 VoxQueue".into());
        }
        let mut state = self.0.lock().unwrap();
        if matches!(state.snapshot.phase.as_str(), "waiting" | "flashing" | "restoring" | "connecting") {
            return Err("已有烧录操作，请等待它结束".into());
        }
        state.cancelled = false;
        state.verified_images = 0;
        state.snapshot = FlashSnapshot {
            phase: "waiting".into(),
            progress: None,
            message:
                "正在等待下载串口。请将键盘关机再开机；若一直等待，在开机状态短按并松开 BOOT 一次。"
                    .into(),
            log: vec![
                "三份镜像 SHA-256 校验通过；恢复引导程序、分区表和应用，保留配网与声音资源。"
                    .into(),
            ],
        };
        let snapshot = state.snapshot.clone();
        drop(state);
        let worker = self.clone();
        std::thread::spawn(move || {
            if let Err(error) = worker.run(&resources) {
                worker.finish("failed", &error);
            }
        });
        Ok(snapshot)
    }

    fn finish(&self, phase: &str, message: &str) {
        let mut state = self.0.lock().unwrap();
        state.snapshot.phase = phase.into();
        state.snapshot.message = message.into();
    }

    fn append_log(&self, line: String) {
        let mut state = self.0.lock().unwrap();
        if let Some(progress) = parse_progress(&line) {
            state.snapshot.progress =
                Some(((state.verified_images.min(2) as u16 * 100 + progress as u16) / 3) as u8);
        }
        if line.contains("Hash of data verified") {
            state.verified_images = (state.verified_images + 1).min(IMAGES.len());
            state.snapshot.progress = Some((state.verified_images * 100 / IMAGES.len()) as u8);
        }
        if state.snapshot.log.len() >= 180 {
            state.snapshot.log.remove(0);
        }
        state.snapshot.log.push(line);
    }

    #[cfg(windows)]
    fn run(&self, resources: &Path) -> Result<(), String> {
        use std::io::Read;
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        let deadline = Instant::now() + Duration::from_secs(120);
        let (port, serial) = loop {
            if self.0.lock().unwrap().cancelled {
                self.finish("cancelled", "已取消等待，未写入固件");
                return Ok(());
            }
            let ports = serial_ports()?;
            if ports.len() > 1 {
                return Err("发现多个 ESP32-S3，请只连接要更新的键盘".into());
            }
            if let Some(port) = ports.into_iter().next() {
                break port;
            }
            if Instant::now() >= deadline {
                return Err(
                    "等待串口超时。请检查 USB 数据线，重新开始后将键盘关机、开机一次".into(),
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        // Recheck all resources after the wait, before changing any device byte.
        validate_restore(resources, &restore_sha())?;
        // Serialize this transition with cancel_wait: cancellation must never race a write.
        {
            let mut state = self.0.lock().unwrap();
            if state.cancelled {
                state.snapshot.phase = "cancelled".into();
                state.snapshot.message = "已取消等待，未写入固件".into();
                return Ok(());
            }
            state.snapshot.phase = "flashing".into();
            state.snapshot.message = "正在写入固件，请勿关闭程序、断电或拔线".into();
        }
        self.append_log(format!("已识别 {port} · 设备 {serial}"));
        let mut child = Command::new(resources.join("esptool/esptool.exe"))
            .args(write_arguments(resources, &port))
            .env("PYTHONUNBUFFERED", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|e| format!("无法启动烧录工具：{e}"))?;
        let (sender, receiver) = std::sync::mpsc::channel::<String>();
        let mut readers = Vec::new();
        // Both pipes are drained concurrently, including progress separated by CR.
        fn read_lines(mut stream: impl Read, sender: std::sync::mpsc::Sender<String>) {
            let mut buffer = [0u8; 1024];
            let mut line = Vec::new();
            while let Ok(count) = stream.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                for byte in &buffer[..count] {
                    if matches!(*byte, b'\r' | b'\n') {
                        if !line.is_empty() {
                            let _ = sender.send(String::from_utf8_lossy(&line).into_owned());
                            line.clear();
                        }
                    } else if line.len() < 4096 {
                        line.push(*byte);
                    }
                }
            }
            if !line.is_empty() {
                let _ = sender.send(String::from_utf8_lossy(&line).into_owned());
            }
        }
        let stdout = child.stdout.take().unwrap();
        let output_sender = sender.clone();
        readers.push(std::thread::spawn(move || {
            read_lines(stdout, output_sender)
        }));
        let stderr = child.stderr.take().unwrap();
        readers.push(std::thread::spawn(move || read_lines(stderr, sender)));
        let deadline = Instant::now() + Duration::from_secs(180);
        let mut verified = 0;
        let exit = loop {
            while let Ok(line) = receiver.try_recv() {
                verified += usize::from(line.contains("Hash of data verified"));
                self.append_log(line);
            }
            match child.try_wait() {
                Ok(Some(exit)) => break exit,
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("读取烧录结果失败：{e}"));
                }
                Ok(None) => {}
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("烧录超时，请查看日志并重新烧录后再使用键盘".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        for reader in readers {
            let _ = reader.join();
        }
        for line in receiver.try_iter() {
            verified += usize::from(line.contains("Hash of data verified"));
            self.append_log(line);
        }
        if !exit.success() || verified != IMAGES.len() {
            return Err("完整恢复未完成：需要三份镜像全部校验通过，请查看下方日志".into());
        }
        self.0.lock().unwrap().snapshot.progress = Some(100);
        self.append_log("固件三段写入完成，正在自动恢复配网配置。".into());
        self.restore_configuration();
        Ok(())
    }

    pub fn retry_configuration(&self) -> Result<FlashSnapshot, String> {
        if !recovery_configured() { return Err("请先保存配网恢复配置".into()); }
        let mut state = self.0.lock().unwrap();
        if matches!(state.snapshot.phase.as_str(), "waiting" | "flashing" | "restoring" | "connecting") { return Err("恢复正在进行，请等待".into()); }
        state.snapshot.phase="restoring".into(); state.snapshot.message="正在恢复配网配置，请保持 USB 连接".into();
        let snapshot=state.snapshot.clone(); drop(state);
        let worker=self.clone(); std::thread::spawn(move ||worker.restore_configuration());
        Ok(snapshot)
    }
    #[cfg(windows)]
    fn restore_configuration(&self) {
        self.finish("restoring", "正在等待键盘启动，通过 USB 自动恢复配网配置");
        let paths=std::env::var_os("LOCALAPPDATA").map(|root|easy_codex_host::paths::AppPaths::from_root(PathBuf::from(root).join(easy_codex_host::paths::APP_SUPPORT_DIRECTORY)));
        let before=paths.as_ref().and_then(|paths|easy_codex_host::health::query_dashboard(&paths.runtime_directory.join(easy_codex_host::health::HEALTH_SOCKET_NAME)).ok()).map(|snapshot|snapshot.lan.heartbeat_authenticated).unwrap_or(0);
        let deadline=Instant::now()+Duration::from_secs(30);
        loop {
            match easy_codex_host::provisioning::restore_saved_lan() {
                Ok(_) => break,
                Err(easy_codex_host::provisioning::ProvisioningError::DeviceNotFound) if Instant::now()<deadline => std::thread::sleep(Duration::from_millis(500)),
                Err(_) => { self.finish("configuration_failed", "固件未受影响，但配网恢复未完成。请连接一块键盘后点击恢复配网"); return; }
            }
        }
        self.append_log("USB 已确认配置保存成功，正在检查键盘心跳。".into());
        self.finish("connecting", "配网已恢复，正在等待键盘连接 Host");

        let deadline=Instant::now()+Duration::from_secs(30);
        while Instant::now()<deadline {
            if let Some(paths)=&paths {
                if let Ok(snapshot)=easy_codex_host::health::query_dashboard(&paths.runtime_directory.join(easy_codex_host::health::HEALTH_SOCKET_NAME)) {
                    if snapshot.lan.heartbeat_authenticated > before && snapshot.lan.keyboard_connected && snapshot.lan.keyboard_firmware_version.as_deref()==Some(easy_codex_host::lan_voice::CURRENT_FIRMWARE_VERSION) {
                        self.finish("completed", "恢复完成：配网已自动写回，键盘已连接，版本上报正常"); return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        self.finish("configuration_failed", "配置已保存，但键盘尚未连接。请检查 Wi-Fi 后点击恢复配网；无需重烧固件");
    }
    #[cfg(not(windows))]
    fn restore_configuration(&self) { self.finish("configuration_failed", "自动配网恢复仅支持 Windows"); }

    #[cfg(not(windows))]
    fn run(&self, _: &Path) -> Result<(), String> {
        Err("当前烧录功能支持 Windows".into())
    }
}

fn validate_restore(resources: &Path, reviewed_sha: &str) -> Result<(), String> {
    if reviewed_sha != restore_sha() {
        return Err("恢复包版本已变化，请刷新页面后重新确认".into());
    }
    for image in IMAGES {
        let bytes = std::fs::read(resources.join(image.file))
            .map_err(|_| format!("安装包缺少{}，请重新安装 VoxQueue", image.name))?;
        if bytes.is_empty()
            || bytes.len() > image.max_size
            || format!("{:x}", Sha256::digest(&bytes)) != image.sha256
        {
            return Err(format!(
                "{}校验失败，已停止恢复；请重新安装 VoxQueue",
                image.name
            ));
        }
    }
    Ok(())
}

fn parse_progress(line: &str) -> Option<u8> {
    if !line.starts_with("Writing at ") {
        return None;
    }
    line.rsplit_once('(')?
        .1
        .split('%')
        .next()?
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|n| *n <= 100)
}

#[cfg(windows)]
pub fn serial_ports() -> Result<Vec<(String, String)>, String> {
    use std::{ffi::c_void, ptr};
    type Key = *mut c_void;
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegOpenKeyExW(
            key: Key,
            subkey: *const u16,
            options: u32,
            access: u32,
            result: *mut Key,
        ) -> i32;
        fn RegEnumKeyExW(
            key: Key,
            index: u32,
            name: *mut u16,
            len: *mut u32,
            reserved: *mut u32,
            class: *mut u16,
            class_len: *mut u32,
            time: *mut c_void,
        ) -> i32;
        fn RegQueryValueExW(
            key: Key,
            name: *const u16,
            reserved: *mut u32,
            kind: *mut u32,
            data: *mut u8,
            size: *mut u32,
        ) -> i32;
        fn RegCloseKey(key: Key) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn QueryDosDeviceW(name: *const u16, target: *mut u16, length: u32) -> u32;
    }
    #[link(name = "cfgmgr32")]
    unsafe extern "system" {
        fn CM_Locate_DevNodeW(node: *mut u32, device_id: *const u16, flags: u32) -> u32;
        fn CM_Get_Parent(parent: *mut u32, node: u32, flags: u32) -> u32;
        fn CM_Get_Device_IDW(node: u32, buffer: *mut u16, length: u32, flags: u32) -> u32;
        fn CM_Get_DevNode_Status(status: *mut u32, problem: *mut u32, node: u32, flags: u32)
        -> u32;
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    struct RegistryKey(Key);
    impl Drop for RegistryKey {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
    unsafe fn open(parent: Key, name: &str) -> Option<RegistryKey> {
        let mut key = ptr::null_mut();
        (unsafe { RegOpenKeyExW(parent, wide(name).as_ptr(), 0, 0x20019, &mut key) } == 0)
            .then_some(RegistryKey(key))
    }
    // Read only known ESP32-S3 USB-Serial/JTAG entries; stale registry devices are
    // excluded by their active DOS mapping. No generic COM port is ever selected.
    unsafe {
        let mut ports = Vec::new();
        // Windows may expose CDC on MI_00 rather than the composite parent.
        for usb_id in ["VID_303A&PID_1001", "VID_303A&PID_1001&MI_00"] {
            let Some(usb) = open(
                (-2147483646isize) as Key,
                &format!("SYSTEM\\CurrentControlSet\\Enum\\USB\\{usb_id}"),
            ) else {
                continue;
            };
            for index in 0..256 {
                let mut name = [0u16; 260];
                let mut length = 260;
                let result = RegEnumKeyExW(
                    usb.0,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                if result == 259 {
                    break;
                }
                if result != 0 {
                    return Err("无法读取 USB 串口信息".into());
                }
                let serial = String::from_utf16_lossy(&name[..length as usize]);
                // A historical COM number can be reused by a different USB device.
                // Require this exact ESP USB devnode to be present and started too.
                let mut node = 0;
                let mut status = 0;
                let mut problem = 0;
                let device_id = wide(&format!("USB\\{usb_id}\\{serial}"));
                if CM_Locate_DevNodeW(&mut node, device_id.as_ptr(), 0) != 0
                    || CM_Get_DevNode_Status(&mut status, &mut problem, node, 0) != 0
                    || status & 0x8 == 0
                {
                    continue;
                }
                let Some(parameters) = open(usb.0, &format!("{serial}\\Device Parameters")) else {
                    continue;
                };
                let mut value = [0u16; 64];
                let mut size = 128;
                let mut kind = 0;
                if RegQueryValueExW(
                    parameters.0,
                    wide("PortName").as_ptr(),
                    ptr::null_mut(),
                    &mut kind,
                    value.as_mut_ptr().cast(),
                    &mut size,
                ) != 0
                    || kind != 1
                {
                    continue;
                }
                let port = String::from_utf16_lossy(
                    &value[..value.iter().position(|n| *n == 0).unwrap_or(value.len())],
                );
                if !valid_port(&port) {
                    continue;
                }
                let mut target = [0u16; 512];
                if QueryDosDeviceW(wide(&port).as_ptr(), target.as_mut_ptr(), 512) > 0 {
                    let serial = if usb_id.ends_with("&MI_00") {
                        let mut parent = 0;
                        let mut id = [0u16; 512];
                        if CM_Get_Parent(&mut parent, node, 0) != 0
                            || CM_Get_Device_IDW(parent, id.as_mut_ptr(), 512, 0) != 0
                        {
                            continue;
                        }
                        let id = String::from_utf16_lossy(
                            &id[..id.iter().position(|n| *n == 0).unwrap_or(id.len())],
                        );
                        let Some(serial) = id.strip_prefix("USB\\VID_303A&PID_1001\\") else {
                            continue;
                        };
                        serial.to_owned()
                    } else {
                        serial
                    };
                    ports.push((port, serial));
                }
            }
        }
        ports.sort();
        ports.dedup();
        Ok(ports)
    }
}

fn valid_port(port: &str) -> bool {
    port.strip_prefix("COM")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_is_bounded_and_only_from_flash_lines() {
        assert_eq!(parse_progress("Writing at 0x10000... (54 %)"), Some(54));
        assert_eq!(parse_progress("Writing at 0x10000... (101 %)"), None);
        assert_eq!(parse_progress("Compressed (99 %)"), None);
    }
    fn reviewed_resources() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        for (source, image) in [
            "bootloader-20261005.bin",
            "partition-table-20261005.bin",
            "version-reporting-r3-20261006.bin",
        ]
        .into_iter()
        .zip(IMAGES)
        {
            std::fs::copy(
                root.join("firmware/releases").join(source),
                dir.path().join(image.file),
            )
            .unwrap();
        }
        dir
    }
    #[test]
    fn all_reviewed_images_must_match_before_any_write() {
        let resources = reviewed_resources();
        assert!(validate_restore(resources.path(), &restore_sha()).is_ok());
        assert!(validate_restore(resources.path(), FIRMWARE_SHA).is_err());
        for image in IMAGES {
            let path = resources.path().join(image.file);
            let original = std::fs::read(&path).unwrap();
            let mut changed = original.clone();
            changed[0] ^= 1;
            std::fs::write(&path, changed).unwrap();
            assert!(validate_restore(resources.path(), &restore_sha()).is_err());
            std::fs::write(&path, &original).unwrap();
            std::fs::remove_file(&path).unwrap();
            assert!(validate_restore(resources.path(), &restore_sha()).is_err());
            std::fs::write(&path, original).unwrap();
        }
    }
    #[test]
    fn complete_restore_writes_three_ranges_and_preserves_user_data() {
        let resources = reviewed_resources();
        let args = write_arguments(resources.path(), "COM7");
        assert!(!args.iter().any(|a| a.to_string_lossy().contains("erase")));
        let image_args = &args[19..];
        assert_eq!(image_args.len(), 6);
        for (pair, image) in image_args.chunks_exact(2).zip(IMAGES) {
            assert_eq!(pair[0].to_string_lossy(), format!("0x{:x}", image.address));
            assert_eq!(Path::new(&pair[1]), resources.path().join(image.file));
            let len = std::fs::metadata(resources.path().join(image.file))
                .unwrap()
                .len() as u32;
            let erased_end = (image.address + len + 4095) & !4095;
            for (start, end) in [(0x9000, 0x10000), (0x310000, 0x430000)] {
                assert!(erased_end <= start || image.address >= end);
            }
        }
        let table = std::fs::read(resources.path().join(IMAGES[1].file)).unwrap();
        let mut apps = Vec::new();
        for row in table
            .chunks_exact(32)
            .take_while(|r| r[..2] == [0xaa, 0x50])
        {
            if row[2] == 0 {
                apps.push((row[3], u32::from_le_bytes(row[4..8].try_into().unwrap())));
            }
            assert!(
                !(row[2] == 1 && row[3] == 0),
                "OTA selection must not remain in the restored table"
            );
        }
        assert_eq!(apps, vec![(0, 0x10000)]);
    }
    #[test]
    fn multi_image_progress_does_not_finish_at_bootloader_100_percent() {
        let flasher = FirmwareFlasher::default();
        flasher.append_log("Writing at 0x00000000... (100 %)".into());
        assert_eq!(flasher.snapshot().progress, Some(33));
        flasher.append_log("Hash of data verified.".into());
        flasher.append_log("Writing at 0x00008000... (100 %)".into());
        assert_eq!(flasher.snapshot().progress, Some(66));
        flasher.append_log("Hash of data verified.".into());
        flasher.append_log("Writing at 0x00010000... (50 %)".into());
        assert_eq!(flasher.snapshot().progress, Some(83));
    }
    #[test]
    fn cannot_cancel_a_write_and_retains_bounded_log() {
        let flasher = FirmwareFlasher::default();
        flasher.0.lock().unwrap().snapshot.phase = "flashing".into();
        assert!(flasher.cancel_wait().is_err());
        assert!(flasher.busy());
        for phase in ["restoring", "connecting"] {
            flasher.0.lock().unwrap().snapshot.phase=phase.into();
            assert!(flasher.busy());
            assert!(flasher.cancel_wait().is_err());
        }
        for i in 0..500 {
            flasher.append_log(i.to_string());
        }
        assert_eq!(flasher.snapshot().log.len(), 180);
    }
    #[test]
    fn stale_ports_are_never_used_as_arbitrary_names() {
        assert!(valid_port("COM7"));
        for port in ["COM", "COM7 --erase-all", "LPT1", "COM../"] {
            assert!(!valid_port(port));
        }
    }
}

fn recovery_configured() -> bool {
    #[cfg(windows)] { easy_codex_host::provisioning::load_recovery_profile().is_ok() }
    #[cfg(not(windows))] { false }
}

fn recovery_network() -> Option<(String,String)> {
    #[cfg(windows)] { easy_codex_host::provisioning::recovery_network_settings() }
    #[cfg(not(windows))] { None }
}
