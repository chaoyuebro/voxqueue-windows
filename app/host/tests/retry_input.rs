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
