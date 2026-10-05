//! Read the board's USB status without displaying its configuration or secrets.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = hidapi::HidApi::new()?;
    let candidates: Vec<_> = api
        .device_list()
        .filter(|item| {
            item.vendor_id() == 0x303a
                && item.product_id() == 0x1006
                && item.interface_number() == 0
                && item.usage_page() == 0xff00
                && item.usage() == 2
        })
        .collect();
    if candidates.len() != 1 {
        return Err(format!("expected one USB management interface, got {}", candidates.len()).into());
    }
    let device = candidates[0].open_device(&api)?;
    let request_id = 0x9a1276b5u32;
    let mut request = [0u8; 17];
    request[0] = 0x13;
    request[1..5].copy_from_slice(b"S3R\x01");
    request[5..9].copy_from_slice(&request_id.to_le_bytes());
    device.send_feature_report(&request)?;

    let mut json = Vec::new();
    let mut next_index = 0u8;
    let mut expected_chunks = None;
    let mut expected_bytes = None;
    for _ in 0..30 {
        let mut report = [0u8; 64];
        let length = device.read_timeout(&mut report, 200)?;
        if length < 14 || report[0] != 0x11 || report[1] != 0x04 {
            continue;
        }
        let chunk_index = report[2];
        let chunk_count = report[3];
        let data_len = report[4] as usize;
        let response_id = u32::from_le_bytes(report[6..10].try_into()?);
        if response_id != request_id || report[5] != 1 || data_len < 9 || data_len > 59
            || length < 14 + data_len - 9 || chunk_count == 0 {
            continue;
        }
        if chunk_index != next_index || expected_chunks.is_some_and(|count| count != chunk_count) {
            return Err("board status chunk sequence is invalid".into());
        }
        expected_chunks = Some(chunk_count);
        expected_bytes = Some(u16::from_le_bytes([report[10], report[11]]) as usize);
        json.extend_from_slice(&report[14..14 + data_len - 9]);
        next_index += 1;
        if next_index == chunk_count {
            break;
        }
    }
    if json.is_empty() || Some(json.len()) != expected_bytes || Some(next_index) != expected_chunks {
        return Err("board did not return a complete status response".into());
    }
    let status: serde_json::Value = serde_json::from_slice(&json)?;
    println!("status_reply=ok");
    println!("firmware={}", status.get("firmware").and_then(|value| value.as_str()).unwrap_or("unknown"));
    println!("config_saved={}", status.get("saved").and_then(|value| value.as_bool()).unwrap_or(false));
    println!("config_bytes={}", status.get("bytes").and_then(|value| value.as_u64()).unwrap_or(0));
    let host = status.pointer("/audio/host").and_then(|value| value.as_str()).unwrap_or("missing");
    println!("audio_host={host}");
    println!("host_matches={}", host == "192.168.31.135");
    println!("audio_port={}", status.pointer("/audio/port").and_then(|value| value.as_u64()).unwrap_or(0));
    for field in ["capture", "stream_phase", "stop_reason", "control_state", "last_error"] {
        if let Some(value) = status.pointer(&format!("/audio/{field}")).and_then(|value| value.as_str()) {
            println!("audio_{field}={value}");
        }
    }
    for field in ["sent_packets", "send_errors", "read_errors", "recovery_count", "session_generation"] {
        if let Some(value) = status.pointer(&format!("/audio/{field}")).and_then(|value| value.as_u64()) {
            println!("audio_{field}={value}");
        }
    }
    Ok(())
}
