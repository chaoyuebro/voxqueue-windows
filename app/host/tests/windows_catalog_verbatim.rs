#![cfg(windows)]

use std::fs;

use easy_codex_host::codex_catalog::CodexTaskCatalog;
use rusqlite::{Connection, params};
use tempfile::tempdir;

#[test]
fn windows_verbatim_rollout_path_remains_in_catalog() {
    let temp = tempdir().unwrap();
    let home = temp.path();
    let sessions = home.join("sessions");
    fs::create_dir(&sessions).unwrap();
    fs::write(
        home.join(".codex-global-state.json"),
        br#"{"pinned-thread-ids":[],"electron-persisted-atom-state":{}}"#,
    )
    .unwrap();
    fs::write(home.join("session_index.jsonl"), b"").unwrap();

    let id = "00000000-0000-4000-8000-000000000001";
    let rollout = sessions.join(format!("{id}.jsonl"));
    fs::write(&rollout, b"").unwrap();
    let verbatim = format!(r"\\?\{}", rollout.display());
    let connection = Connection::open(home.join("state_5.sqlite")).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY, name TEXT, title TEXT NOT NULL,
                cwd TEXT NOT NULL, rollout_path TEXT NOT NULL,
                updated_at INTEGER NOT NULL, updated_at_ms INTEGER,
                recency_at_ms INTEGER NOT NULL, archived INTEGER NOT NULL,
                source TEXT, thread_source TEXT, agent_role TEXT
            );",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO threads
             (id, name, title, cwd, rollout_path, updated_at,
              updated_at_ms, recency_at_ms, archived, source, thread_source, agent_role)
             VALUES (?1, 'Slot 1', '', ?2, ?3, 1, 1000, 1000, 0, 'exec', 'user', NULL)",
            params![id, home.to_string_lossy(), verbatim],
        )
        .unwrap();
    drop(connection);

    let catalog = CodexTaskCatalog::from_paths(home.to_path_buf(), home.join("snapshots"));
    let task = catalog.allowlisted(id).unwrap();
    assert_eq!(task.rollout_path, rollout);

    // Bound tasks must survive recency eviction while new bindings remain list-gated.
    let connection = Connection::open(home.join("state_5.sqlite")).unwrap();
    for index in 2..=12 {
        let newer = format!("00000000-0000-4000-8000-{index:012}");
        let newer_rollout = sessions.join(format!("{newer}.jsonl"));
        fs::write(&newer_rollout, b"").unwrap();
        connection.execute("INSERT INTO threads VALUES (?1, 'Newer', '', ?2, ?3, 2, 2000, 2000, 0, 'exec', 'user', NULL)",
            params![newer, home.to_string_lossy(), newer_rollout.to_string_lossy()]).unwrap();
    }
    assert!(catalog.allowlisted(id).is_err());
    assert_eq!(catalog.bound_task(id).unwrap().task_id, id);
    connection.execute("UPDATE threads SET archived=1 WHERE id=?1", [id]).unwrap();
    assert!(catalog.bound_task(id).is_err());
}
