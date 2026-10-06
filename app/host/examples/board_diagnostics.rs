//! Read-only USB firmware/network diagnostics. Never print configuration secrets.
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = hidapi::HidApi::new()?;
    let info = api
        .device_list()
        .find(|d| {
            d.vendor_id() == 0x303a
                && d.product_id() == 0x1006
                && d.usage_page() == 0xff00
                && d.usage() == 2
        })
        .ok_or("USB device missing")?;
    let device = info.open_device(&api)?;
    let id = 0x564f5801u32;
    let mut request = [0u8; 17];
    request[0] = 0x13;
    request[1..4].copy_from_slice(b"S3R");
    request[4] = 1;
    request[5..9].copy_from_slice(&id.to_le_bytes());
    request[9] = 1;
    device.send_feature_report(&request)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut chunks = BTreeMap::new();
    while Instant::now() < deadline {
        let mut report = [0u8; 64];
        let size = device.read_timeout(&mut report, 200)?;
        if size != 64
            || report[0] != 0x11
            || report[1] != 4
            || report[5] != 1
            || u32::from_le_bytes(report[6..10].try_into()?) != id
        {
            continue;
        }
        let total = report[3] as usize;
        let length = report[4] as usize;
        if total == 0 || total > 128 || report[2] as usize >= total || !(9..=59).contains(&length) {
            continue;
        }
        chunks.insert(report[2], report[14..5 + length].to_vec());
        if chunks.len() == total {
            let bytes = chunks.values().flatten().copied().collect::<Vec<_>>();
            let expected = u16::from_le_bytes(report[10..12].try_into()?) as usize;
            if bytes.len() != expected {
                return Err("Incomplete response".into());
            }
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            for pointer in [
                "/config/audio",
                "/config/network",
                "/config/wifi",
                "/runtime/audio",
                "/capabilities",
            ] {
                if let Some(v) = value.pointer(pointer) {
                    for sub in [
                        "enabled",
                        "configured",
                        "connected",
                        "host",
                        "port",
                        "mode",
                        "ready",
                    ] {
                        if let Some(safe) = v.get(sub) {
                            println!("{pointer}.{sub}={safe}");
                        }
                    }
                }
            }
            for key in [
                "firmware",
                "phase",
                "status",
                "target_platform",
                "saved",
                "bytes",
                "hotkey_mode",
                "ptt_hotkey",
                "edit_ptt_hotkey",
                "audio",
                "network",
                "wifi",
                "capabilities",
            ] {
                if let Some(v) = value.get(key) {
                    if v.is_object() {
                        for sub in [
                            "enabled",
                            "configured",
                            "connected",
                            "host",
                            "port",
                            "state",
                            "ready",
                            "key_valid",
                            "wifi_connected",
                            "wifi_configured",
                        ] {
                            if let Some(safe) = v.get(sub) {
                                println!("{key}.{sub}={safe}");
                            }
                        }
                    } else if v.is_string() || v.is_boolean() || v.is_number() {
                        println!("{key}={v}");
                    }
                }
            }
            return Ok(());
        }
    }
    Err("USB status timeout".into())
}
