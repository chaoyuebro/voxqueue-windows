//! Bind a CLI task to the running Host; read its ID from stdin to avoid command arguments.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let slot: u8 = std::env::args().nth(1).ok_or("missing slot")?.parse()?;
    let mut task_id = String::new();
    std::io::stdin().read_line(&mut task_id)?;
    let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is unavailable")?;
    let paths = easy_codex_host::paths::AppPaths::from_root(
        std::path::PathBuf::from(local).join(easy_codex_host::paths::APP_SUPPORT_DIRECTORY),
    );
    let socket = paths.runtime_directory.join(easy_codex_host::health::HEALTH_SOCKET_NAME);
    let current = easy_codex_host::health::query_dashboard(&socket)?;
    let expected = current.slots.iter().find(|item| item.slot == slot).ok_or("slot missing")?.binding_generation;
    let updated = easy_codex_host::health::bind_dashboard_slot(&socket, slot, task_id.trim(), expected)?;
    let bound = updated.slots.iter().any(|item| item.slot == slot && item.task_id.as_deref() == Some(task_id.trim()));
    println!("slot={slot} bound={bound}");
    if !bound { return Err("binding did not take effect".into()); }
    Ok(())
}
