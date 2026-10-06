//! Read the current Host provider and LAN counters without exposing task content.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is unavailable")?;
    let paths = easy_codex_host::paths::AppPaths::from_root(
        std::path::PathBuf::from(local).join(easy_codex_host::paths::APP_SUPPORT_DIRECTORY),
    );
    let snapshot = easy_codex_host::health::query_dashboard(
        &paths.runtime_directory.join(easy_codex_host::health::HEALTH_SOCKET_NAME),
    )?;
    println!("provider_region={}", snapshot.provider.region);
    println!("configured={}", snapshot.provider.configured);
    println!("asr_model={}", snapshot.provider.asr_model);
    println!("tts_model={}", snapshot.provider.tts_model);
    println!("voice={}", snapshot.provider.voice);
    println!("device_key_loaded={}", snapshot.lan.auth_key_loaded);
    println!("udp_received={}", snapshot.lan.udp_received);
    println!("heartbeat_received={}", snapshot.lan.heartbeat_received);
    println!("heartbeat_authenticated={}", snapshot.lan.heartbeat_authenticated);
    println!("keyboard_volume_percent={:?}", snapshot.lan.keyboard_volume_percent);
    println!("preview_supported={}", snapshot.lan.preview_supported);
    println!("preview_busy={}", snapshot.lan.preview_busy);
    println!("preview_status={}", snapshot.lan.preview_status);
    println!("playback_received={}", snapshot.lan.playback_received);
    println!("mailbox_sent={}", snapshot.lan.mailbox_sent);
    println!("audio_frames_accepted={}", snapshot.lan.audio_frames_accepted);
    println!("audio_ends_accepted={}", snapshot.lan.audio_ends_accepted);
    println!("captures_ready={}", snapshot.lan.captures_ready);
    println!("captures_rejected={}", snapshot.lan.captures_rejected);
    println!("audio_auth_rejected={}", snapshot.lan.audio_auth_rejected);
    println!("asr_succeeded={}", snapshot.lan.asr_succeeded);
    println!("asr_failed={}", snapshot.lan.asr_failed);
    println!("prompts_delivered={}", snapshot.lan.prompts_delivered);
    println!("queue_inserted={}", snapshot.lan.queue_inserted);
    for slot in &snapshot.slots {
        println!("slot_{}_bound={}", slot.slot, slot.task_id.is_some());
        println!(
            "slot_{}_selectable={}",
            slot.slot,
            slot.task_id.as_ref().is_some_and(|task_id| {
                snapshot.tasks.iter().any(|task| &task.task_id == task_id)
            })
        );
        println!("slot_{}_pending_jobs={}", slot.slot, slot.pending_jobs);
        println!("slot_{}_unread={}", slot.slot, slot.unread_generation.is_some());
        println!("slot_{}_job_state={}", slot.slot, slot.latest_job_state.as_deref().unwrap_or("none"));
        println!("slot_{}_job_failure={}", slot.slot, slot.latest_job_failure.as_deref().unwrap_or("none"));
    }
    Ok(())
}
