#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(any(target_os = "macos", windows))]
use easy_codex_host::health::{
    DashboardSnapshot, HEALTH_SOCKET_NAME, HealthError, HealthSnapshot, bind_dashboard_slot,
    query_dashboard, query_health,
};
#[cfg(any(target_os = "macos", windows))]
use easy_codex_host::paths::AppPaths;
#[cfg(any(target_os = "macos", windows))]
use serde::Serialize;
#[cfg(windows)]
use tauri::Manager;

#[cfg(any(target_os = "macos", windows))]
#[derive(Debug, Serialize)]
#[serde(tag = "connection", rename_all = "snake_case")]
enum HostProbe {
    Healthy { health: HealthSnapshot },
    Offline { reason: &'static str },
    ProtocolError { reason: &'static str },
}

#[cfg(any(target_os = "macos", windows))]
#[derive(Debug, Serialize)]
#[serde(tag = "connection", rename_all = "snake_case")]
enum DashboardProbe {
    Healthy { dashboard: DashboardSnapshot },
    Offline { reason: &'static str },
    ProtocolError { reason: &'static str },
}

#[cfg(any(target_os = "macos", windows))]
fn app_paths() -> Option<AppPaths> {
    #[cfg(windows)]
    {
        let install_local = std::env::current_exe().ok().and_then(|exe| {
            let install_dir = exe.parent()?;
            if !install_dir
                .file_name()?
                .to_string_lossy()
                .eq_ignore_ascii_case("Codex Keyboard")
                && !install_dir
                    .file_name()?
                    .to_string_lossy()
                    .eq_ignore_ascii_case("easyinput")
                && !install_dir
                    .file_name()?
                    .to_string_lossy()
                    .eq_ignore_ascii_case("VoxQueue")
            {
                return None;
            }
            Some(install_dir.parent()?.to_path_buf())
        });
        install_local
            .or_else(|| std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from))
            .map(|local| AppPaths::from_root(local.join("EasyCodexInput")))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(|home| AppPaths::from_home(std::path::Path::new(&home)))
    }
}

#[cfg(any(target_os = "macos", windows))]
fn is_offline(error: &HealthError) -> bool {
    matches!(
        error,
        HealthError::Io(source)
            if matches!(
                source.kind(),
                std::io::ErrorKind::NotFound
                    | std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::TimedOut
            )
    )
}

#[cfg(any(target_os = "macos", windows))]
#[tauri::command]
fn host_health() -> HostProbe {
    let Some(paths) = app_paths() else {
        return HostProbe::Offline {
            reason: "home_unavailable",
        };
    };
    match query_health(&paths.runtime_directory.join(HEALTH_SOCKET_NAME)) {
        Ok(health) => HostProbe::Healthy { health },
        Err(error) => {
            #[cfg(windows)]
            let _ = std::fs::write(
                paths.runtime_directory.join("desktop-last-error.txt"),
                format!("health: {error:?}"),
            );
            if is_offline(&error) {
                HostProbe::Offline { reason: "host_unreachable" }
            } else {
                HostProbe::ProtocolError { reason: "health_invalid" }
            }
        },
    }
}

#[cfg(any(target_os = "macos", windows))]
#[tauri::command]
fn host_dashboard() -> DashboardProbe {
    let Some(paths) = app_paths() else {
        return DashboardProbe::Offline {
            reason: "home_unavailable",
        };
    };
    match query_dashboard(&paths.runtime_directory.join(HEALTH_SOCKET_NAME)) {
        Ok(dashboard) => DashboardProbe::Healthy { dashboard },
        Err(error) if is_offline(&error) => DashboardProbe::Offline {
            reason: "host_unreachable",
        },
        Err(error) => {
            #[cfg(not(windows))]
            eprintln!("dashboard_probe_failed error={error}");
            #[cfg(windows)]
            let _ = error;
            DashboardProbe::ProtocolError {
                reason: "dashboard_invalid",
            }
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
#[tauri::command]
fn bind_slot(
    slot: u8,
    task_id: String,
    expected_generation: Option<u64>,
) -> Result<DashboardSnapshot, &'static str> {
    let Some(paths) = app_paths() else {
        return Err("home_unavailable");
    };
    bind_dashboard_slot(
        &paths.runtime_directory.join(HEALTH_SOCKET_NAME),
        slot,
        &task_id,
        expected_generation,
    )
    .map_err(|error| match error {
        HealthError::Rejected(_) => "binding_rejected",
        error if is_offline(&error) => "host_unreachable",
        _ => "dashboard_invalid",
    })
}

#[cfg(any(target_os = "macos", windows))]
#[tauri::command]
fn clear_summary_queue(slot: u8, expected_generation: u64) -> Result<DashboardSnapshot, &'static str> {
    let paths = app_paths().ok_or("home_unavailable")?;
    easy_codex_host::health::clear_dashboard_summary_queue(
        &paths.runtime_directory.join(HEALTH_SOCKET_NAME), slot, expected_generation)
        .map_err(|_| "clear_queue_failed")
}

#[cfg(any(target_os = "macos", windows))]
#[tauri::command]
fn open_codex_task(task_id: String) -> Result<(), &'static str> {
    let task_id = uuid::Uuid::parse_str(&task_id).map_err(|_| "invalid_task_id")?;
    let url = format!("codex://threads/{task_id}");
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "Shell32")]
        unsafe extern "system" {
            fn ShellExecuteW(
                hwnd: *mut std::ffi::c_void,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show: i32,
            ) -> isize;
        }
        #[link(name = "User32")]
        unsafe extern "system" {
            fn FindWindowW(class_name: *const u16, window_name: *const u16)
            -> *mut std::ffi::c_void;
            fn ShowWindow(window: *mut std::ffi::c_void, command: i32) -> i32;
            fn SetForegroundWindow(window: *mut std::ffi::c_void) -> i32;
        }
        let operation = std::ffi::OsStr::new("open")
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let url = std::ffi::OsStr::new(&url)
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if result <= 32 {
            return Err("open_failed");
        }
        // Protocol activation may navigate an existing Codex window without
        // raising it. The click in this window grants foreground permission.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let title = std::ffi::OsStr::new("ChatGPT")
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if !window.is_null() {
            unsafe {
                ShowWindow(window, 9);
                SetForegroundWindow(window);
            }
        }
    }
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|_| "open_failed")?;
    Ok(())
}

#[cfg(any(target_os = "macos", windows))]
fn main() {
    tauri::Builder::default()
        .manage(firmware_flash::FirmwareFlasher::default())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                use tauri::Manager;
                // Keep the write alive. The UI exposes cancellation only while waiting.
                if window.state::<firmware_flash::FirmwareFlasher>().busy() { api.prevent_close(); }
            }
        })
        .setup(|app| {
            #[cfg(windows)]
            {
                let Some(paths) = app_paths() else { return Ok(()); };
                let endpoint = paths.runtime_directory.join(HEALTH_SOCKET_NAME);
                if matches!(query_health(&endpoint), Err(ref error) if is_offline(error)) {
                    use std::os::windows::process::CommandExt;
                    use std::process::{Command, Stdio};

                    let host = app.path().resource_dir()?.join("easy-codex-host.exe");
                    if !host.is_file() {
                        eprintln!("bundled_host=missing");
                    } else if let Err(error) = Command::new(host)
                        .arg("daemon")
                        .env("LOCALAPPDATA", paths.root.parent().unwrap_or(&paths.root))
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .creation_flags(0x0800_0000 | 0x0000_0200)
                        .spawn()
                    {
                        eprintln!("bundled_host=start_failed error={error}");
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            host_health,
            host_dashboard,
            bind_slot,
            clear_summary_queue,
            retry_failed_input,
            preview_answer_voice,
            answer_voice_settings,
            save_answer_voice_settings,
            open_codex_task,
            firmware_info,
            firmware_flash_status,
            start_firmware_flash,
            cancel_firmware_flash
        ])
        .run(tauri::generate_context!())
        .expect("VoxQueue desktop runtime failed");
}

#[cfg(not(any(target_os = "macos", windows)))]
fn main() {
    println!("VoxQueue desktop requires macOS or Windows");
}
#[cfg(any(target_os = "macos", windows))]
mod firmware_flash;

#[cfg(any(target_os = "macos", windows))]
#[derive(Serialize)]
struct AnswerVoicePreferences {
    settings: easy_codex_host::voice_settings::VoiceSettings,
    voices: Vec<easy_codex_host::voice_settings::VoiceOption>,
}

#[cfg(any(target_os = "macos", windows))]
#[tauri::command]
fn answer_voice_settings() -> Result<AnswerVoicePreferences, String> {
    let paths = app_paths().ok_or("数据目录不可用")?;
    Ok(AnswerVoicePreferences {
        settings: easy_codex_host::voice_settings::VoiceSettings::load(&paths.root).map_err(|e| e.to_string())?,
        voices: easy_codex_host::voice_settings::voices(),
    })
}

#[cfg(any(target_os = "macos", windows))]
#[tauri::command]
fn save_answer_voice_settings(settings: easy_codex_host::voice_settings::VoiceSettings) -> Result<easy_codex_host::voice_settings::VoiceSettings, String> {
    let paths = app_paths().ok_or("数据目录不可用")?;
    settings.save(&paths.root).map_err(|e| e.to_string())?;
    Ok(settings)
}

#[cfg(any(target_os = "macos", windows))]
#[tauri::command]
fn firmware_info(app: tauri::AppHandle) -> Result<firmware_flash::FirmwareInfo, String> {
    use tauri::Manager;
    Ok(firmware_flash::info(&app.path().resource_dir().map_err(|e| e.to_string())?))
}

#[cfg(any(target_os = "macos", windows))]
#[tauri::command]
fn firmware_flash_status(state: tauri::State<'_, firmware_flash::FirmwareFlasher>) -> firmware_flash::FlashSnapshot {
    state.snapshot()
}

#[cfg(any(target_os = "macos", windows))]
#[tauri::command]
fn start_firmware_flash(app: tauri::AppHandle, state: tauri::State<'_, firmware_flash::FirmwareFlasher>, expected_sha256: String) -> Result<firmware_flash::FlashSnapshot, String> {
    use tauri::Manager;
    state.start(app.path().resource_dir().map_err(|e| e.to_string())?, expected_sha256)
}

#[cfg(any(target_os = "macos", windows))]
#[tauri::command]
fn cancel_firmware_flash(state: tauri::State<'_, firmware_flash::FirmwareFlasher>) -> Result<(), String> {
    state.cancel_wait()
}

#[tauri::command]
fn retry_failed_input(slot: u8, expected_generation: u64, original: String) -> Result<DashboardSnapshot, String> {
    let paths = app_paths().ok_or("home_unavailable")?;
    easy_codex_host::health::retry_dashboard_input(&paths.runtime_directory.join(HEALTH_SOCKET_NAME), slot, expected_generation, original)
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
#[tauri::command]
async fn preview_answer_voice(settings: easy_codex_host::voice_settings::VoiceSettings) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if BUSY.swap(true, std::sync::atomic::Ordering::AcqRel) { return Err("正在生成试听，请稍候".into()); }
        struct Reset;
        impl Drop for Reset { fn drop(&mut self) { BUSY.store(false, std::sync::atomic::Ordering::Release); } }
        let _reset = Reset;
        settings.validate().map_err(|e| e.to_string())?;
        let paths = app_paths().ok_or("home_unavailable")?;
        let socket = paths.runtime_directory.join(HEALTH_SOCKET_NAME);
        // A reboot clears Host diagnostics until the first authenticated heartbeat.
        // Wait through two heartbeat periods before diagnosing firmware support.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let dashboard = loop {
            let snapshot = query_dashboard(&socket).map_err(|_| "Host 尚未就绪，请稍后重试".to_string())?;
            if snapshot.lan.preview_supported && snapshot.lan.keyboard_volume_percent.is_some() {
                break snapshot;
            }
            if std::time::Instant::now() >= deadline {
                if snapshot.lan.keyboard_volume_percent.is_none() {
                    return Err("尚未收到键盘的有效心跳，请等待键盘联网后重试".into());
                }
                return Err("键盘已连接，但上报的固件尚不支持试听，请检查固件版本".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        };
        if dashboard.lan.preview_busy { return Err("键盘正在播放或已有试听待播，请结束后再试听".into()); }
        let key = easy_codex_host::windows_credential::read_minimax_key()
            .map_err(|e| e.to_string())?.ok_or("请先配置语音服务")?;
        let client = easy_codex_host::minimax::VoiceClient::new().map_err(|e| e.to_string())?;
        let audio = client.synthesize_with_settings(&key, "你好，这是 VoxQueue 播报试听。你可以调整音色、语速，并转动旋钮调节音量。", &settings)
            .map_err(|_| "试听生成失败，请检查语音服务后重试".to_string())?;
        let encoded = easy_codex_host::audio::encode_tts_audio(audio.pcm()).map_err(|e| e.to_string())?;
        let id = uuid::Uuid::new_v4();
        let token = u32::from_le_bytes(id.as_bytes()[..4].try_into().unwrap()) | 0x8000_0000;
        let path = paths.runtime_directory.join(format!("preview-{id}.eiad"));
        std::fs::write(&path, encoded.eiad()).map_err(|e| e.to_string())?;
        let result = easy_codex_host::health::preview_dashboard_audio(&socket, id.to_string(), token);
        let _ = std::fs::remove_file(path);
        result.map_err(|e| e.to_string())?;
        Ok("试听已准备，键盘将在空闲时播放（等待最多 30 秒）；可用旋钮调节音量".into())
    }).await.map_err(|e| e.to_string())?
}
