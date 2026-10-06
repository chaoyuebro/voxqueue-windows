use easy_codex_host::{audio, lan_playback::*, lan_voice::{LanVoiceConfig, LanVoiceIngress}, paths::AppPaths};
use std::{net::UdpSocket, time::{Duration, Instant}};
use hmac::{Hmac, Mac};
use sha2::Sha256;

#[test]
fn preview_is_authenticated_streamed_acknowledged_and_does_not_emit_summary_events() {
    run_preview(false);
}

#[test]
fn rejected_preview_reports_failure() {
    run_preview(true);
}

fn run_preview(reject: bool) {
    let temp = tempfile::tempdir().unwrap();
    let key = [7; 32];
    let mut config = LanVoiceConfig::from_paths(&AppPaths::from_root(temp.path().join("root"))); config.bind_port = 0; config.auth_key = Some(key);
    let ingress = LanVoiceIngress::start(config).unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let target = format!("127.0.0.1:{}", ingress.local_port());
    let mut heartbeat = [0u8; 80];
    heartbeat[..4].copy_from_slice(b"EIHB"); heartbeat[4]=1; heartbeat[5]=2; heartbeat[6]=5; heartbeat[7]=3;
    heartbeat[20..24].copy_from_slice(b"EISD"); heartbeat[24]=1; heartbeat[25]=60; heartbeat[26]=2;
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).unwrap();
    mac.update(b"EasyInput/EISD/v1"); mac.update(&heartbeat[..64]);
    heartbeat[64..].copy_from_slice(&mac.finalize().into_bytes()[..16]);
    socket.send_to(&heartbeat, &target).unwrap();
    let mut buffer = [0u8; 2048]; socket.recv_from(&mut buffer).unwrap();
    let deadline = Instant::now()+Duration::from_secs(1);
    while !ingress.diagnostics().preview_supported && Instant::now()<deadline { std::thread::sleep(Duration::from_millis(5)); }
    assert_eq!(ingress.diagnostics().keyboard_firmware_version.as_deref(), Some(easy_codex_host::lan_voice::CURRENT_FIRMWARE_VERSION));
    let pcm = vec![0u8; 9600];
    let encoded = audio::encode_tts_audio(&pcm).unwrap();
    let token = 0x8000_0123;
    assert!(ingress.preview_audio(token, zeroize::Zeroizing::new(encoded.eiad().to_vec())));
    assert_eq!(ingress.diagnostics().preview_status, "waiting");
    assert!(!ingress.preview_audio(token+1, zeroize::Zeroizing::new(encoded.eiad().to_vec())));
    socket.send_to(&heartbeat, &target).unwrap();
    let (size, _) = socket.recv_from(&mut buffer).unwrap();
    assert_eq!(size, 36); assert_eq!(buffer[4], 5);
    assert_eq!(u32::from_le_bytes(buffer[16..20].try_into().unwrap()), token);
    let request = PlaybackRequest { slot: 1, request_generation: token, connection_generation: 1, nonce: 42 };
    let mut bad = encode_request(request, &key); bad[8] ^= 1;
    socket.send_to(&bad, &target).unwrap();
    socket.send_to(&encode_request(request, &key), &target).unwrap();
    let (size, _) = socket.recv_from(&mut buffer).unwrap();
    let begin = decode_begin(&buffer[..size], &key).unwrap();
    assert_eq!(begin.identity.request_generation, token);
    wait_status(&ingress, "streaming");
    if reject {
        socket.send_to(&encode_ack(PlaybackAck { identity: begin.identity, status: 0, next_offset: 0 }, &key), &target).unwrap();
        let (size, _) = socket.recv_from(&mut buffer).unwrap();
        assert!(decode_data(&buffer[..size], 42, &key).is_ok());
        socket.send_to(&encode_ack(PlaybackAck { identity: begin.identity, status: 2, next_offset: 0 }, &key), &target).unwrap();
        wait_status(&ingress, "failed");
        return;
    }
    socket.send_to(&encode_ack(PlaybackAck { identity: begin.identity, status: 0, next_offset: 0 }, &key), &target).unwrap();
    let mut collected = Vec::new();
    while collected.len() < begin.total_bytes as usize {
        let (size, _) = socket.recv_from(&mut buffer).unwrap();
        let (identity, offset, data) = decode_data(&buffer[..size], 42, &key).unwrap();
        assert_eq!(identity, begin.identity);
        if offset as usize == collected.len() { collected.extend(data); }
        socket.send_to(&encode_ack(PlaybackAck { identity, status: 0, next_offset: collected.len() as u32 }, &key), &target).unwrap();
    }
    assert_eq!(collected.as_slice(), audio::transcode_eiad_for_device(encoded.eiad()).unwrap().as_slice());
    socket.send_to(&encode_finished(PlaybackFinished { identity: begin.identity, played_samples: begin.total_samples }, &key), &target).unwrap();
    loop {
        let (size, _) = socket.recv_from(&mut buffer).unwrap();
        if &buffer[..4] == b"EIPK" { assert_eq!(decode_finished_ack(&buffer[..size], &key).unwrap(), (begin.identity, 0)); break; }
    }
    wait_status(&ingress, "completed");
    assert!(ingress.try_recv_playback().is_none());
}

fn wait_status(ingress: &LanVoiceIngress, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while ingress.diagnostics().preview_status != expected && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(5)); }
    assert_eq!(ingress.diagnostics().preview_status, expected);
}
