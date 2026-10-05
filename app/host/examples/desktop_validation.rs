//! Explicit, harmless desktop integration check against an existing test conversation.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use easy_codex_host::codex_catalog::CodexTaskCatalog;
    use easy_codex_host::codex_runner::{CodexRunner, CodexRunnerConfig};
    use easy_codex_host::prompt_queue::DurablePromptScheduler;
    use easy_codex_host::store::{NewJob, StateStore};
    use rusqlite::Connection;
    use std::time::{Duration, Instant};
    let ids: Vec<String> = std::env::args().skip(1).collect();
    if ids.len() != 1 && ids.len() != 4 {
        return Err("provide one existing test thread or four test threads".into());
    }
    let catalog = CodexTaskCatalog::from_environment()?;
    let temp = tempfile::tempdir()?;
    let state = temp.path().join("state.sqlite3");
    let journal = temp.path().join("desktop-delivery.sqlite3");
    let mut store = StateStore::open(&state)?;
    let tag = uuid::Uuid::new_v4().to_string();
    for slot in 1..=4 {
        let id = &ids[if ids.len() == 1 { 0 } else { slot - 1 }];
        let task = catalog.bound_task(id)?;
        store.set_binding(slot as u8, None, id)?;
        store.enqueue(&NewJob { request_id: &format!("{tag}-{slot}"), task_id: id,
            slot: slot as u8, generation: 1, cwd: &task.cwd,
            prompt: &format!("VoxQueue Rust 桌面接入测试 {tag} 槽位{slot}。只回复 VOXQUEUE_RUST_SLOT_{slot}_OK，不使用工具，不修改文件。") })?;
    }
    let config = CodexRunnerConfig {
        timeout: Duration::from_secs(180),
        ..Default::default()
    };
    let mut scheduler =
        DurablePromptScheduler::new(CodexRunner::new(config).with_desktop(journal.clone()));
    let mut observer = easy_codex_host::rollout_observer::RolloutObserver::new(catalog);
    let deadline = Instant::now() + Duration::from_secs(240);
    let db = Connection::open(&state)?;
    loop {
        scheduler.tick(&mut store)?;
        observer.tick_due(&mut store)?;
        let pending: u32 = db.query_row(
            "SELECT COUNT(*) FROM jobs WHERE state IN ('queued','running')",
            [],
            |r| r.get(0),
        )?;
        if pending == 0 {
            break;
        }
        if Instant::now() > deadline {
            return Err("desktop validation timeout".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut stmt = db.prepare("SELECT slot,state,failure_kind FROM jobs ORDER BY sequence")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, u8>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut all = true;
    for row in rows {
        let (slot, state, failure) = row?;
        all &= state == "completed";
        println!("slot={slot} state={state} failure={failure:?}");
    }
    let delivered = Connection::open(&journal)?;
    let turns: u32 = delivered.query_row(
        "SELECT COUNT(DISTINCT turn_id) FROM deliveries WHERE state='completed'",
        [],
        |r| r.get(0),
    )?;
    println!("completed distinct turns={turns}; tag={tag}");
    if !all || turns != 4 {
        return Err("four separate completed desktop turns required".into());
    }
    for _ in 0..20 {
        observer.poll_bound_tasks(&mut store)?;
    }
    let mut stmt = delivered.prepare("SELECT turn_id FROM deliveries WHERE state='completed'")?;
    let turn_ids = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut observed = 0;
    for id in turn_ids {
        let count: u32 = db.query_row(
            "SELECT COUNT(*) FROM completion_ledger WHERE completion_id=?1",
            [id?],
            |r| r.get(0),
        )?;
        observed += count;
    }
    println!("authoritative completion ledger entries={observed}");
    if observed != 4 {
        for id in &ids {
            let task = easy_codex_host::codex_catalog::CodexTaskCatalog::from_environment()?
                .bound_task(id)?;
            println!(
                "observer diagnostic {id}: {:?}",
                observer.poll_task(&mut store, &task)
            );
        }
        println!("diagnostic state preserved at {}", temp.keep().display());
        return Err("completion observer did not record all four desktop turns".into());
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows desktop only");
}
