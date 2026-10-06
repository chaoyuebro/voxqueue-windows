use easy_codex_host::{lan_voice::{LanVoiceConfig, LanVoiceIngress}, paths::AppPaths};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{net::UdpSocket, time::{Duration, Instant}};

fn packet(level: u8, version: u8, key: &[u8; 32]) -> [u8; 80] {
    let mut packet = [0_u8; 80];
    packet[..4].copy_from_slice(b"EIHB");
    packet[4] = 1; packet[5] = 2; packet[6] = level; packet[7] = version;
    packet[20..24].copy_from_slice(b"EISD");
    packet[24] = 1; packet[25] = 60; packet[26] = 2;
    let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
    mac.update(b"EasyInput/EISD/v1"); mac.update(&packet[..64]);
    packet[64..].copy_from_slice(&mac.finalize().into_bytes()[..16]);
    packet
}

#[test]
fn authenticated_udp_updates_volume_rejects_tampering_and_expires() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(temp.path().join("app"));
    let key = [7; 32];
    let mut config = LanVoiceConfig::from_paths(&paths);
    config.bind_port = 0; config.auth_key = Some(key);
    let ingress = LanVoiceIngress::start(config).unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    let target = ("127.0.0.1", ingress.local_port());
    assert_eq!(ingress.diagnostics().keyboard_volume_percent, None);
    for level in [0, 3, 10] {
        socket.send_to(&packet(level, 1, &key), target).unwrap();
        let mut reply = [0; 128]; socket.recv_from(&mut reply).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while ingress.diagnostics().keyboard_volume_percent != Some(level * 10) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(ingress.diagnostics().keyboard_volume_percent, Some(level * 10));
    }
    assert!(ingress.diagnostics().keyboard_firmware_version.is_none());
    socket.send_to(&packet(10, 4, &key), target).unwrap();
    let mut reply = [0; 128]; socket.recv_from(&mut reply).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(ingress.diagnostics().keyboard_firmware_version.as_deref(), Some(easy_codex_host::lan_voice::CURRENT_FIRMWARE_VERSION));
    assert!(ingress.diagnostics().keyboard_connected);
    socket.send_to(&packet(10, 3, &key), target).unwrap();
    socket.recv_from(&mut reply).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(ingress.diagnostics().keyboard_firmware_version.as_deref(), Some("2026.10.06-r3"));
    let count = ingress.diagnostics().heartbeat_authenticated;
    let mut altered = packet(5, 1, &key); altered[6] = 7;
    for invalid in [altered, packet(11, 1, &key), packet(5, 5, &key)] {
        socket.send_to(&invalid, target).unwrap();
    }
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(ingress.diagnostics().heartbeat_authenticated, count);
    assert_eq!(ingress.diagnostics().keyboard_volume_percent, Some(100));
    std::thread::sleep(Duration::from_secs(12));
    assert_eq!(ingress.diagnostics().keyboard_volume_percent, None);
    assert!(ingress.diagnostics().keyboard_firmware_version.is_none());
    assert!(!ingress.diagnostics().keyboard_connected);
    socket.send_to(&packet(6, 1, &key), target).unwrap();
    let mut reply = [0; 128]; socket.recv_from(&mut reply).unwrap();
    socket.send_to(&packet(0, 0, &key), target).unwrap();
    socket.recv_from(&mut reply).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(ingress.diagnostics().keyboard_volume_percent, None);
}
