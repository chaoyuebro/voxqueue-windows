use easy_codex_host::store::{StateStore, NewJob, JobFailureKind, StoreError};

#[test]
fn retry_keeps_exact_input_is_idempotent_and_rejects_rebinding() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = StateStore::open(&dir.path().join("state.sqlite3")).unwrap();
    store.set_binding(1, None, "task-a").unwrap();
    store.enqueue(&NewJob { request_id: "failed-1", task_id: "task-a", slot: 1,
        generation: 1, prompt: "保留原来的识别文字", cwd: dir.path() }).unwrap();
    let first = store.claim_next_runnable().unwrap().unwrap();
    store.mark_failed(&first.request_id, first.claim_generation, JobFailureKind::DesktopUnavailable).unwrap();
    assert_eq!(store.failed_input("task-a", 1, 1).unwrap().unwrap().1, first.prompt);
    store.retry_failed_input(1, 1, "failed-1").unwrap();
    store.retry_failed_input(1, 1, "failed-1").unwrap();
    assert_eq!(store.pending_count("task-a").unwrap(), 1);
    assert!(store.failed_input("task-a", 1, 1).unwrap().is_none());
    let retry = store.claim_next_runnable().unwrap().unwrap();
    assert_eq!(retry.prompt, first.prompt);
    assert_eq!(retry.cwd, first.cwd);
    assert_ne!(retry.request_id, first.request_id);
    store.set_binding(1, Some(1), "task-b").unwrap();
    assert!(matches!(store.retry_failed_input(1, 1, "failed-1"), Err(StoreError::BindingChanged)));
    assert!(store.retry_failed_input(2, 1, "failed-1").is_err());
}

#[test]
fn clear_dismisses_only_current_slot_failures_persistently_and_allows_new_failures() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.sqlite3");
    let mut store = StateStore::open(&db).unwrap();
    for slot in 1..=2 {
        let task = format!("task-{slot}");
        let request = format!("failed-{slot}");
        store.set_binding(slot, None, &task).unwrap();
        store.enqueue(&NewJob { request_id: &request, task_id: &task, slot, generation: 1, prompt: "保留文字", cwd: dir.path() }).unwrap();
        let job = store.claim_next_runnable().unwrap().unwrap();
        store.mark_failed(&job.request_id, job.claim_generation, JobFailureKind::DesktopUnavailable).unwrap();
    }
    assert!(store.clear_slot_summary_queue(1, 2).is_err());
    assert!(store.failed_input("task-1", 1, 1).unwrap().is_some());
    store.clear_slot_summary_queue(1, 1).unwrap();
    drop(store);
    let mut store = StateStore::open(&db).unwrap();
    assert!(store.failed_input("task-1", 1, 1).unwrap().is_none());
    assert!(store.latest_job_outcome("task-1", 1).unwrap().is_none());
    assert!(store.retry_failed_input(1, 1, "failed-1").is_err());
    assert!(store.failed_input("task-2", 1, 2).unwrap().is_some());
    store.enqueue(&NewJob { request_id: "new-1", task_id: "task-1", slot: 1, generation: 1, prompt: "新文字", cwd: dir.path() }).unwrap();
    let job = store.claim_next_runnable().unwrap().unwrap();
    store.clear_slot_summary_queue(1, 1).unwrap();
    assert_eq!(store.pending_count("task-1").unwrap(), 1);
    store.mark_failed(&job.request_id, job.claim_generation, JobFailureKind::DesktopUnavailable).unwrap();
    assert_eq!(store.failed_input("task-1", 1, 1).unwrap().unwrap().0, "new-1");
}

#[test]
fn schema_seven_upgrade_preserves_failed_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.sqlite3");
    let mut store = StateStore::open(&db).unwrap();
    store.set_binding(1, None, "task-a").unwrap();
    store.enqueue(&NewJob { request_id: "old-failure", task_id: "task-a", slot: 1, generation: 1, prompt: "升级前文字", cwd: dir.path() }).unwrap();
    let job = store.claim_next_runnable().unwrap().unwrap();
    store.mark_failed(&job.request_id, job.claim_generation, JobFailureKind::DesktopUnavailable).unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection.execute_batch("ALTER TABLE jobs DROP COLUMN failure_dismissed; PRAGMA user_version=7;").unwrap();
    drop(connection);
    let mut store = StateStore::open(&db).unwrap();
    assert_eq!(store.schema_version().unwrap(), 8);
    assert_eq!(store.failed_input("task-a", 1, 1).unwrap().unwrap().1, "升级前文字");
    store.clear_slot_summary_queue(1, 1).unwrap();
    assert!(store.failed_input("task-a", 1, 1).unwrap().is_none());
}
