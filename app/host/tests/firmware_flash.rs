#![cfg(windows)]
#[path = "../../desktop/src-tauri/src/firmware_flash.rs"]
mod firmware_flash;

#[test]
fn serial_probe_excludes_unrelated_and_disconnected_ports() {
    let ports = firmware_flash::serial_ports().unwrap();
    assert!(ports.iter().all(|(port, serial)| port.starts_with("COM") && !serial.is_empty()));
}
