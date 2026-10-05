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
                recency_at_ms INTEGER, archived INTEGER NOT NULL,
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

    // All user tasks remain bindable, including old tasks and those never opened recently.
    let connection = Connection::open(home.join("state_5.sqlite")).unwrap();
    for index in 2..=105 {
        let newer = format!("00000000-0000-4000-8000-{index:012}");
        let newer_rollout = sessions.join(format!("{newer}.jsonl"));
        fs::write(&newer_rollout, b"").unwrap();
        connection.execute("INSERT INTO threads VALUES (?1, 'Newer', '', ?2, ?3, 2, 2000, 2000, 0, 'exec', 'user', NULL)",
            params![newer, home.to_string_lossy(), newer_rollout.to_string_lossy()]).unwrap();
    }
    connection.execute("UPDATE threads SET recency_at_ms=NULL WHERE id=?1", [id]).unwrap();
    assert_eq!(catalog.list_tasks().unwrap().len(), 105);
    assert_eq!(catalog.allowlisted(id).unwrap().task_id, id);
    assert_eq!(catalog.bound_task(id).unwrap().task_id, id);
    connection.execute("UPDATE threads SET archived=1 WHERE id=?1", [id]).unwrap();
    assert!(catalog.bound_task(id).is_err());
    assert!(catalog.allowlisted(id).is_err());
    assert_eq!(catalog.list_tasks().unwrap().len(), 104);
}

#[test]
#[ignore = "Reads the current user's local Codex catalog"]
fn live_catalog_includes_older_sidebar_tasks() {
    let home = std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".codex");
    let temp = tempdir().unwrap();
    let catalog = CodexTaskCatalog::from_paths(home, temp.path().join("snapshots"));
    let tasks = catalog.list_tasks().unwrap();
    for id in ["01a0b8af-743d-7b01-ae23-ad37c7c8050c", "01a08690-f58b-7730-aadd-81700a098ab0", "01a0869b-c55e-7ba0-ab71-7c3ba6642cc8"] {
        assert!(tasks.iter().any(|task| task.task_id == id), "missing sidebar task {id}");
    }
    println!("local_catalog_tasks={} older_sidebar_tasks=present", tasks.len());
}

#[test]
#[ignore = "Queries a running Host specified by VOXQUEUE_TEST_ROOT"]
fn live_dashboard_transports_full_task_list() {
    use easy_codex_host::health::{query_dashboard, HEALTH_SOCKET_NAME};
    let root = std::path::PathBuf::from(std::env::var_os("VOXQUEUE_TEST_ROOT").unwrap());
    let dashboard = query_dashboard(&root.join("run").join(HEALTH_SOCKET_NAME)).unwrap();
    assert!(dashboard.tasks.len() > 8);
    assert!(dashboard.tasks.iter().any(|task| task.task_id == "01a0b8af-743d-7b01-ae23-ad37c7c8050c"));
    println!("installed_host_dashboard_tasks={} older_sidebar_task=present", dashboard.tasks.len());
}
