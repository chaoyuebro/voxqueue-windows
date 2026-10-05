use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const FIRMWARE_SHA: &str = "d7670a545bd35461e0fd6cf3f04d24a4ec1f3ffae64f54924ee3a5a18a7a1cc0";

#[derive(Clone, Serialize)]
pub struct FirmwareInfo {
    pub name: &'static str,
    pub sha256: &'static str,
    pub available: bool,
}

pub fn info(resources: &Path) -> FirmwareInfo {
    FirmwareInfo {
        name: "2026.10.05 · 桌面软件运行指示灯",
        sha256: FIRMWARE_SHA,
        available: cfg!(windows)
            && resources.join("current-firmware.bin").is_file()
            && resources.join("esptool/esptool.exe").is_file(),
    }
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
}

#[derive(Clone, Default)]
pub struct FirmwareFlasher(Arc<Mutex<State>>);

impl FirmwareFlasher {
    pub fn snapshot(&self) -> FlashSnapshot {
        self.0.lock().unwrap().snapshot.clone()
    }

    pub fn busy(&self) -> bool {
        matches!(self.snapshot().phase.as_str(), "waiting" | "flashing")
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
        let firmware = resources.join("current-firmware.bin");
        validate_image(&firmware, &reviewed_sha)?;
        if !resources.join("esptool/esptool.exe").is_file() {
            return Err("安装包缺少烧录工具，请重新安装 VoxQueue".into());
        }
        let mut state = self.0.lock().unwrap();
        if matches!(state.snapshot.phase.as_str(), "waiting" | "flashing") {
            return Err("已有烧录操作，请等待它结束".into());
        }
        state.cancelled = false;
        state.snapshot = FlashSnapshot {
            phase: "waiting".into(),
            progress: None,
            message: "请将键盘电源关机，再开机一次；不用按 BOOT。正在等待烧录串口…".into(),
            log: vec!["固件 SHA-256 校验通过；仅更新应用，保留配网与声音资源。".into()],
        };
        let snapshot = state.snapshot.clone();
        drop(state);
        let worker = self.clone();
        std::thread::spawn(move || {
            if let Err(error) = worker.run(&resources, &firmware) {
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
            state.snapshot.progress = Some(progress);
        }
        if state.snapshot.log.len() >= 180 {
            state.snapshot.log.remove(0);
        }
        state.snapshot.log.push(line);
    }

    #[cfg(windows)]
    fn run(&self, resources: &Path, firmware: &Path) -> Result<(), String> {
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
            .args([
                "--chip",
                "esp32s3",
                "--port",
                &port,
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
                "0x10000",
            ])
            .arg(firmware)
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
        let mut verified = false;
        let exit = loop {
            while let Ok(line) = receiver.try_recv() {
                verified |= line.contains("Hash of data verified");
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
            verified |= line.contains("Hash of data verified");
            self.append_log(line);
        }
        if !exit.success() || !verified {
            return Err("烧录未完成或校验失败，请查看下方日志".into());
        }
        self.0.lock().unwrap().snapshot.progress = Some(100);
        self.finish(
            "completed",
            "烧录成功，写入校验通过。键盘已自动重启，配网数据保留。",
        );
        Ok(())
    }

    #[cfg(not(windows))]
    fn run(&self, _: &Path, _: &Path) -> Result<(), String> {
        Err("当前烧录功能支持 Windows".into())
    }
}

fn validate_image(path: &Path, reviewed_sha: &str) -> Result<(), String> {
    if reviewed_sha != FIRMWARE_SHA {
        return Err("固件版本已变化，请刷新页面后重新确认".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "安装包缺少当前固件，请重新安装 VoxQueue")?;
    if bytes.len() > 0x300000
        || bytes.len() < 65536
        || format!("{:x}", Sha256::digest(&bytes)) != FIRMWARE_SHA
    {
        return Err("固件校验失败，已停止烧录；请重新安装 VoxQueue".into());
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
        let Some(usb) = open(
            (-2147483646isize) as Key,
            "SYSTEM\\CurrentControlSet\\Enum\\USB\\VID_303A&PID_1001",
        ) else {
            return Ok(vec![]);
        };
        let mut ports = Vec::new();
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
            let device_id = wide(&format!("USB\\VID_303A&PID_1001\\{serial}"));
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
                ports.push((port, serial));
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
    #[test]
    fn rejects_tampered_and_unreviewed_firmware() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("image.bin");
        std::fs::write(&file, vec![0u8; 65536]).unwrap();
        assert!(validate_image(&file, FIRMWARE_SHA).is_err());
        assert!(validate_image(&file, "other-version").is_err());
    }
    #[test]
    fn cannot_cancel_a_write_and_retains_bounded_log() {
        let flasher = FirmwareFlasher::default();
        flasher.0.lock().unwrap().snapshot.phase = "flashing".into();
        assert!(flasher.cancel_wait().is_err());
        assert!(flasher.busy());
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
