#![cfg(windows)]

use std::collections::BTreeSet;
use std::fs;

use easy_codex_host::cache::{CacheBundle, CacheError, CacheId, CacheLimits, CacheStore};
use easy_codex_host::codex_catalog::{CodexTask, CodexTaskCatalog};
use easy_codex_host::paths::{AppPaths, open_private_file, replace_private_file};
use easy_codex_host::rollout_observer::RolloutObserver;
use easy_codex_host::secrets::{ImportLock, KeychainAccounts, LocalCacheSecretStore};
use easy_codex_host::store::{NewJob, StateStore};
use rusqlite::{Connection, params};
use serde_json::json;

#[test]
fn per_slot_clear_preserves_other_queues_fences_workers_and_preserves_new_work() {
    use easy_codex_host::store::SummaryClaimResult;
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("state.sqlite3");
    let mut store = StateStore::open(&path).unwrap();
    let connection = Connection::open(&path).unwrap();
    let tasks = (0..5)
        .map(|_| uuid::Uuid::new_v4().to_string())
        .collect::<Vec<_>>();
    let insert = |task: &str| {
        let id = uuid::Uuid::new_v4().to_string();
        connection
            .execute(
                "INSERT INTO completion_ledger
            (completion_id,task_id,rollout_cursor,observed_at,turn_pack)
            VALUES (?1,?2,'fixture',unixepoch(),'{\"turn\":1}')",
                params![id, task],
            )
            .unwrap();
        id
    };
    for (index, task) in tasks.iter().enumerate() {
        insert(task);
        if index < 4 {
            store.set_binding(index as u8 + 1, None, task).unwrap();
        }
    }
    let Some(SummaryClaimResult::Claimed(active)) =
        store.claim_summary(&tasks[0], "active").unwrap()
    else {
        panic!()
    };
    for index in 2..4 {
        let Some(SummaryClaimResult::Claimed(claim)) = store
            .claim_summary(&tasks[index], &format!("published-{index}"))
            .unwrap()
        else {
            panic!()
        };
        store
            .publish_summary(
                &claim,
                &CacheId::for_task(&tasks[index], 1).unwrap().reference(),
            )
            .unwrap();
    }
    let lease = store.acquire_summary_playback(3, 1, 1, 1).unwrap().unwrap();
    store
        .enqueue(&NewJob {
            request_id: "queued-job",
            task_id: &tasks[0],
            slot: 1,
            generation: 1,
            prompt: "keep this task",
            cwd: temporary.path(),
        })
        .unwrap();
    assert!(store.clear_slot_summary_queue(1, 2).is_err());
    assert_eq!(
        store.pending_summary_completion_count(&tasks[0]).unwrap(),
        1
    );
    store.clear_slot_summary_queue(1, 1).unwrap();
    assert_eq!(
        store.pending_summary_completion_count(&tasks[1]).unwrap(),
        1
    );
    assert!(store.current_unread_summary(&tasks[2]).unwrap().is_some());
    assert!(store.current_unread_summary(&tasks[3]).unwrap().is_some());
    for slot in 2..=4 {
        store.clear_slot_summary_queue(slot, 1).unwrap();
    }
    for task in &tasks[..4] {
        assert!(store.current_unread_summary(task).unwrap().is_none());
        assert_eq!(store.pending_summary_completion_count(task).unwrap(), 0);
        assert!(
            store
                .claim_summary(task, &format!("after-clear-{task}"))
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(store.mailbox_status().unwrap().unread_slots, 0);
    assert_eq!(
        store.pending_summary_completion_count(&tasks[4]).unwrap(),
        1
    );
    assert_eq!(store.pending_count(&tasks[0]).unwrap(), 1);
    assert!(
        store
            .publish_summary(
                &active,
                &CacheId::for_task(&tasks[0], 1).unwrap().reference()
            )
            .is_err()
    );
    assert!(!store.cancel_summary_playback(&lease).unwrap());
    assert!(!store.finish_summary_playback(&lease).unwrap());
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM summary_ledger", [], |r| r.get(0))
        .unwrap();
    for slot in 1..=4 {
        store.clear_slot_summary_queue(slot, 1).unwrap();
    }
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM summary_ledger", [], |r| r.get(0))
            .unwrap(),
        count
    );
    drop(store);
    let mut store = StateStore::open(&path).unwrap();
    assert_eq!(store.mailbox_status().unwrap().unread_slots, 0);
    assert!(
        store
            .resume_interrupted_summary(&tasks[0])
            .unwrap()
            .is_none()
    );
    let new_id = insert(&tasks[0]);
    let Some(SummaryClaimResult::Claimed(fresh)) = store.claim_summary(&tasks[0], "fresh").unwrap()
    else {
        panic!()
    };
    assert_eq!(fresh.completions.len(), 1);
    assert_eq!(fresh.completions[0].completion_id, new_id);
    assert!(fresh.previous_unread.is_none());
    store
        .publish_summary(
            &fresh,
            &CacheId::for_task(&tasks[0], fresh.generation)
                .unwrap()
                .reference(),
        )
        .unwrap();
    assert_eq!(store.mailbox_status().unwrap().unread_slots, 1);
}

#[test]
fn interrupted_summary_recovers_after_previous_audio_was_heard() {
    use easy_codex_host::store::SummaryClaimResult;
    const TASK: &str = "019fa972-5cfa-75e1-9008-0b17ade9a347";
    const FIRST: &str = "019fa972-5cfa-75e1-9008-0b17ade9a348";
    const SECOND: &str = "019fa972-5cfa-75e1-9008-0b17ade9a349";
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("state.sqlite3");
    let mut store = StateStore::open(&path).unwrap();
    let connection = Connection::open(&path).unwrap();
    let insert = |id: &str| {
        connection.execute(
            "INSERT INTO completion_ledger
             (completion_id, task_id, rollout_cursor, observed_at, turn_pack)
             VALUES (?1, ?2, 'fixture', unixepoch(), '{\"turn\":1}')",
            rusqlite::params![id, TASK],
        ).unwrap();
    };
    insert(FIRST);
    let Some(SummaryClaimResult::Claimed(first)) = store.claim_summary(TASK, "first").unwrap() else {
        panic!("expected first claim");
    };
    store.publish_summary(&first, &CacheId::for_task(TASK, 1).unwrap().reference()).unwrap();
    insert(SECOND);
    let Some(SummaryClaimResult::Claimed(second)) = store.claim_summary(TASK, "second").unwrap() else {
        panic!("expected second claim");
    };
    store.begin_summary_tts_attempt(&second).unwrap();
    store.mark_summary_tts_ambiguous(&second).unwrap();
    connection.execute("UPDATE summary_ledger SET state = 'heard' WHERE task_id = ?1 AND generation = 1", [TASK]).unwrap();
    drop(store);
    let mut reopened = StateStore::open(&path).unwrap();
    assert!(reopened.resume_interrupted_summary(TASK).unwrap().is_none());
    assert!(reopened.summary_tts_attempt(&second).unwrap().is_none());
    assert_eq!(reopened.pending_summary_completion_count(TASK).unwrap(), 1);
    let Some(SummaryClaimResult::Claimed(replacement)) = reopened.claim_summary(TASK, "replacement").unwrap() else {
        panic!("expected replacement claim");
    };
    assert_eq!(replacement.generation, 3);
    assert!(replacement.previous_unread.is_none());
    assert_eq!(replacement.completions[0].completion_id, SECOND);
}

#[test]
fn failed_codex_turn_does_not_block_later_authoritative_completion() {
    const TASK: &str = "019fa972-5cfa-75e1-9008-0b17ade9a347";
    const FAILED: &str = "019fa972-5cfa-75e1-9008-0b17ade9a348";
    const SUCCEEDED: &str = "019fa972-5cfa-75e1-9008-0b17ade9a349";
    let temporary = tempfile::tempdir().unwrap();
    let rollout = temporary.path().join("rollout.jsonl");
    let records = [
        json!({"type":"session_meta","payload":{"id":TASK}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":FAILED}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":FAILED}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":SUCCEEDED}}),
        json!({"type":"response_item","payload":{
            "type":"message","role":"user",
            "content":[{"type":"input_text","text":"please confirm"}],
            "internal_chat_message_metadata_passthrough":{"turn_id":SUCCEEDED}
        }}),
        json!({"type":"event_msg","payload":{
            "type":"task_complete","turn_id":SUCCEEDED,
            "last_agent_message":"confirmed"
        }}),
    ];
    let contents = records
        .into_iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    fs::write(&rollout, contents).unwrap();
    let task = CodexTask {
        task_id: TASK.to_owned(),
        name: "fixture".to_owned(),
        project: "fixture".to_owned(),
        cwd: temporary.path().to_path_buf(),
        rollout_path: rollout.clone(),
        updated_at_ms: 1,
        pinned: false,
        cli_created: true,
    };
    let catalog = CodexTaskCatalog::from_paths(
        temporary.path().join("unused-codex"),
        temporary.path().join("unused-snapshots"),
    );
    let state_path = temporary.path().join("state.sqlite3");
    let mut store = StateStore::open(&state_path).unwrap();
    let completions = RolloutObserver::new(catalog)
        .poll_task(&mut store, &task)
        .unwrap();
    assert_eq!(completions.len(), 1);
    let connection = Connection::open(state_path).unwrap();
    let ids = connection
        .prepare("SELECT completion_id FROM completion_ledger ORDER BY completion_id")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(ids, vec![SUCCEEDED]);
    assert_eq!(store.rollout_cursor(TASK).unwrap().unwrap().offset, fs::metadata(rollout).unwrap().len());
}

#[test]
fn windows_absolute_task_path_can_be_queued_and_read_back() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(temporary.path().join("private"));
    paths.prepare().unwrap();
    let cwd = temporary.path().join("task-one");
    fs::create_dir(&cwd).unwrap();
    let mut store = StateStore::open(&paths.state_database).unwrap();
    store
        .enqueue(&NewJob {
            request_id: "capture-one",
            task_id: "019fa972-5cfa-75e1-9008-0b17ade9a347",
            slot: 1,
            generation: 1,
            prompt: "hello",
            cwd: &cwd,
        })
        .unwrap();
    let claimed = store.claim_next_runnable().unwrap().unwrap();
    assert_eq!(claimed.cwd, cwd);
    assert_eq!(claimed.prompt, "hello");
}

#[test]
fn private_file_replacement_changes_handle_identity() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(temporary.path().join("private"));
    paths.prepare().unwrap();
    let path = paths.root.join("value.bin");
    replace_private_file(&path, b"first").unwrap();
    let original = open_private_file(&path).unwrap();
    assert!(easy_codex_host::windows_paths::same_file_handle(&original, &path).unwrap());
    replace_private_file(&path, b"second").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"second");
    assert!(!easy_codex_host::windows_paths::same_file_handle(&original, &path).unwrap());
}

#[test]
fn encrypted_cache_publishes_reads_and_reconciles_generation() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_root(temporary.path().join("private"));
    paths.prepare().unwrap();
    let import_lock = ImportLock::acquire(&paths.runtime_directory.join("key-import.lock")).unwrap();
    let accounts = KeychainAccounts::load_or_create(&paths.installation_id, &import_lock).unwrap();
    let secrets = LocalCacheSecretStore::new(paths.cache_secret.clone(), &accounts);
    let cache = CacheStore::initialize(&paths.cache_directory, &secrets, &accounts, CacheLimits::default()).unwrap();
    let id = CacheId::for_task("019fa972-5cfa-75e1-9008-0b17ade9a347", 1).unwrap();
    let bundle = CacheBundle { manifest_json: b"{}", qwen_wav: b"wav", device_eiad: b"eiad" };
    cache.publish(&id, bundle).unwrap();
    let read = cache.read(&id).unwrap();
    assert_eq!(read.manifest_json.as_slice(), bundle.manifest_json);
    assert_eq!(read.qwen_wav.as_slice(), bundle.qwen_wav);
    assert_eq!(read.device_eiad.as_slice(), bundle.device_eiad);
    assert_eq!(cache.audit().unwrap().finalized_generations, 1);
    let ciphertext_path = paths.cache_directory.join(id.reference()).join("qwen.wav.enc");
    let mut ciphertext = fs::read(&ciphertext_path).unwrap();
    *ciphertext.last_mut().unwrap() ^= 1;
    fs::write(&ciphertext_path, &ciphertext).unwrap();
    assert!(matches!(cache.read(&id), Err(CacheError::Corrupt)));
    *ciphertext.last_mut().unwrap() ^= 1;
    fs::write(&ciphertext_path, &ciphertext).unwrap();
    let retained = BTreeSet::from([id.reference()]);
    assert_eq!(cache.reconcile(&retained).unwrap().orphan_generations_removed, 0);
    assert_eq!(cache.reconcile(&BTreeSet::new()).unwrap().orphan_generations_removed, 1);
    assert!(matches!(cache.read(&id), Err(CacheError::MissingObject)));
}
