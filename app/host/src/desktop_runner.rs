//! Windows desktop IPC adapter. Acknowledgement is never treated as completion.
//! The journal is committed before writing a prompt; ambiguous delivery is never replayed.
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::codex_catalog::{CatalogError, CodexTaskCatalog};
use crate::store::{Job, JobFailureKind};

type Result<T> = std::result::Result<T, JobFailureKind>;
const MAX_FRAME: usize = 16 * 1024 * 1024;
const MAX_LINE: usize = 1024 * 1024;
const MAX_ROLLOUT: u64 = 256 * 1024 * 1024;

#[link(name = "Kernel32")]
unsafe extern "system" {
    fn PeekNamedPipe(
        handle: *mut c_void,
        buffer: *mut c_void,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        remaining: *mut u32,
    ) -> i32;
}

struct Ipc {
    pipe: File,
    buffer: Vec<u8>,
    client: String,
}

impl Ipc {
    fn connect(cancel: &AtomicBool, deadline: Instant) -> Result<Self> {
        let pipe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(r"\\.\pipe\codex-ipc")
            .map_err(|_| JobFailureKind::DesktopUnavailable)?;
        let mut ipc = Self {
            pipe,
            buffer: Vec::new(),
            client: "initializing-client".into(),
        };
        let reply = ipc.request(
            "initialize",
            0,
            json!({"clientType":"voxqueue"}),
            None,
            cancel,
            deadline,
        )?;
        ipc.client = reply
            .pointer("/result/clientId")
            .and_then(Value::as_str)
            .ok_or(JobFailureKind::InvalidOutput)?
            .to_owned();
        Ok(ipc)
    }

    fn send(&mut self, value: &Value) -> Result<()> {
        let bytes = serde_json::to_vec(value).map_err(|_| JobFailureKind::InvalidOutput)?;
        if bytes.len() > MAX_FRAME {
            return Err(JobFailureKind::OutputTooLarge);
        }
        self.pipe
            .write_all(&(bytes.len() as u32).to_le_bytes())
            .and_then(|_| self.pipe.write_all(&bytes))
            .map_err(|_| JobFailureKind::ProcessIo)
    }

    fn receive(&mut self, cancel: &AtomicBool, deadline: Instant) -> Result<Value> {
        loop {
            check(cancel, deadline)?;
            if self.buffer.len() >= 4 {
                let length = u32::from_le_bytes(self.buffer[..4].try_into().unwrap()) as usize;
                if length > MAX_FRAME {
                    return Err(JobFailureKind::OutputTooLarge);
                }
                if self.buffer.len() >= 4 + length {
                    let value = serde_json::from_slice(&self.buffer[4..4 + length])
                        .map_err(|_| JobFailureKind::InvalidOutput)?;
                    self.buffer.drain(..4 + length);
                    return Ok(value);
                }
            }
            let mut available = 0;
            let ok = unsafe {
                PeekNamedPipe(
                    self.pipe.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(JobFailureKind::ProcessIo);
            }
            if available == 0 {
                thread::sleep(Duration::from_millis(30));
                continue;
            }
            let mut chunk = vec![0; (available as usize).min(65536)];
            let count = self
                .pipe
                .read(&mut chunk)
                .map_err(|_| JobFailureKind::ProcessIo)?;
            if count == 0 {
                return Err(JobFailureKind::ProcessIo);
            }
            self.buffer.extend_from_slice(&chunk[..count]);
        }
    }

    fn request(
        &mut self,
        method: &str,
        version: u32,
        payload: Value,
        target: Option<&str>,
        cancel: &AtomicBool,
        outer_deadline: Instant,
    ) -> Result<Value> {
        check(cancel, outer_deadline)?;
        let id = Uuid::new_v4().to_string();
        let mut request = json!({"type":"request", "requestId":id, "sourceClientId":self.client,
            "version":version, "method":method, "params":payload, "timeoutMs":30000});
        if let Some(target) = target {
            request["targetClientId"] = json!(target);
        }
        self.send(&request)?;
        let deadline = outer_deadline.min(Instant::now() + Duration::from_secs(32));
        loop {
            let reply = self.receive(cancel, deadline)?;
            if reply["type"] == "client-discovery-request" {
                self.send(
                    &json!({"type":"client-discovery-response", "requestId":reply["requestId"],
                    "response":{"canHandle":false}}),
                )?;
            }
            if reply["type"] == "response" && reply["requestId"] == id {
                if reply["resultType"] != "success" {
                    return Err(JobFailureKind::ActiveSession);
                }
                return Ok(reply);
            }
        }
    }
}

fn check(cancel: &AtomicBool, deadline: Instant) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        return Err(JobFailureKind::ProcessIo);
    }
    if Instant::now() >= deadline {
        return Err(JobFailureKind::Timeout);
    }
    Ok(())
}

/// Incremental JSONL reader. Partial writes stay buffered; only matching lifecycle IDs finish a job.
struct Lifecycle {
    file: File,
    path: std::path::PathBuf,
    identity: (u64, u64),
    first_line: bool,
    session_id: Option<String>,
    partial: Vec<u8>,
    bytes: u64,
    active: Option<String>,
    terminal: BTreeMap<String, bool>,
}

impl Lifecycle {
    fn open(path: &Path) -> Result<Self> {
        let file =
            crate::windows_paths::read_private_file(path).map_err(|_| JobFailureKind::ProcessIo)?;
        let identity =
            crate::windows_paths::file_identity(&file).map_err(|_| JobFailureKind::ProcessIo)?;
        let mut scan = Self {
            file,
            path: path.to_owned(),
            identity,
            first_line: true,
            session_id: None,
            partial: Vec::new(),
            bytes: 0,
            active: None,
            terminal: BTreeMap::new(),
        };
        scan.poll()?;
        Ok(scan)
    }

    fn poll(&mut self) -> Result<()> {
        let current = crate::windows_paths::read_private_file(&self.path)
            .map_err(|_| JobFailureKind::ProcessIo)?;
        if crate::windows_paths::file_identity(&current).map_err(|_| JobFailureKind::ProcessIo)?
            != self.identity
        {
            return Err(JobFailureKind::InvalidOutput);
        }
        if self
            .file
            .metadata()
            .map_err(|_| JobFailureKind::ProcessIo)?
            .len()
            < self.bytes
        {
            return Err(JobFailureKind::InvalidOutput);
        }
        let mut chunk = [0u8; 65536];
        loop {
            let count = self
                .file
                .read(&mut chunk)
                .map_err(|_| JobFailureKind::ProcessIo)?;
            if count == 0 {
                break;
            }
            self.bytes += count as u64;
            if self.bytes > MAX_ROLLOUT {
                return Err(JobFailureKind::OutputTooLarge);
            }
            for &byte in &chunk[..count] {
                if byte == b'\n' {
                    let line = std::mem::take(&mut self.partial);
                    if !line.is_empty() {
                        self.observe(&line)?;
                    }
                } else {
                    if self.partial.len() >= MAX_LINE {
                        return Err(JobFailureKind::OutputTooLarge);
                    }
                    self.partial.push(byte);
                }
            }
        }
        Ok(())
    }

    fn observe(&mut self, line: &[u8]) -> Result<()> {
        let value: Value =
            serde_json::from_slice(line).map_err(|_| JobFailureKind::InvalidOutput)?;
        if self.first_line {
            self.first_line = false;
            if value["type"] == "session_meta" {
                self.session_id = value["payload"]["id"].as_str().map(str::to_owned);
            }
        }
        if value["type"] != "event_msg" {
            return Ok(());
        }
        let payload = &value["payload"];
        let kind = payload["type"].as_str().unwrap_or("");
        if !matches!(kind, "task_started" | "task_complete" | "turn_aborted") {
            return Ok(());
        }
        let id = payload["turn_id"]
            .as_str()
            .ok_or(JobFailureKind::InvalidOutput)?
            .to_owned();
        if kind == "task_started" {
            self.active = Some(id);
        } else {
            if self.active.as_deref() == Some(&id) {
                self.active = None;
            }
            let success = kind == "task_complete"
                && payload["error"].is_null()
                && payload["last_agent_message"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty());
            self.terminal.insert(id, success);
        }
        Ok(())
    }

    fn wait_idle(&mut self, cancel: &AtomicBool, deadline: Instant) -> Result<()> {
        loop {
            check(cancel, deadline)?;
            self.poll()?;
            if self.active.is_none() && self.partial.is_empty() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    fn wait_turn(&mut self, turn: &str, cancel: &AtomicBool, deadline: Instant) -> Result<()> {
        loop {
            check(cancel, deadline)?;
            self.poll()?;
            if let Some(success) = self.terminal.get(turn) {
                return if *success {
                    Ok(())
                } else {
                    Err(JobFailureKind::ExitFailure)
                };
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
}

fn start_payload(job: &Job) -> Value {
    json!({"conversationId":job.task_id, "turnStart":{
        "request":{"threadId":job.task_id, "clientUserMessageId":job.request_id,
            "input":[{"type":"text","text":job.prompt,"text_elements":[]}]},
        "context":{"inheritThreadSettings":true,"attachments":[],"commentAttachments":[]}}})
}

fn journal(path: &Path) -> Result<Connection> {
    let db = Connection::open(path).map_err(|_| JobFailureKind::ProcessIo)?;
    db.busy_timeout(Duration::from_secs(5))
        .map_err(|_| JobFailureKind::ProcessIo)?;
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
        CREATE TABLE IF NOT EXISTS deliveries (
            request_id TEXT PRIMARY KEY, task_id TEXT NOT NULL,
            turn_id TEXT, state TEXT NOT NULL CHECK(state IN ('intent','accepted','completed','aborted')));
        CREATE INDEX IF NOT EXISTS delivery_task ON deliveries(task_id);")
        .map_err(|_| JobFailureKind::ProcessIo)?;
    Ok(db)
}

pub fn run(job: &Job, cancel: &AtomicBool, timeout: Duration, journal_path: &Path) -> Result<()> {
    let deadline = Instant::now() + timeout;
    crate::windows_paths::validate_directory(&job.cwd)
        .map_err(|_| JobFailureKind::UnsafeWorkingDirectory)?;
    let catalog = CodexTaskCatalog::from_environment().map_err(|_| JobFailureKind::ProcessIo)?;
    let catalog_deadline = deadline.min(Instant::now() + Duration::from_secs(30));
    let task = loop {
        check(cancel, catalog_deadline)?;
        match catalog.bound_task(&job.task_id) {
            Ok(task) => break task,
            Err(CatalogError::NotAllowlisted) => return Err(JobFailureKind::TaskArchived),
            Err(CatalogError::IndexTooLarge) => return Err(JobFailureKind::OutputTooLarge),
            Err(_) => thread::sleep(Duration::from_millis(200)),
        }
    };
    if task.cwd != job.cwd {
        return Err(JobFailureKind::UnsafeWorkingDirectory);
    }
    let db = journal(journal_path)?;
    let prior: Option<(String, Option<String>)> = db
        .query_row(
            "SELECT state, turn_id FROM deliveries WHERE request_id=?1",
            [&job.request_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|_| JobFailureKind::ProcessIo)?;
    if matches!(&prior, Some((state, _)) if state == "completed") {
        return Ok(());
    }
    if matches!(&prior, Some((state, _)) if state == "aborted") {
        return Err(JobFailureKind::ExitFailure);
    }
    if matches!(&prior, Some((state, _)) if state == "intent") {
        return Err(JobFailureKind::DeliveryUncertain);
    }
    // A recovered pre-upgrade CLI claim has no desktop delivery evidence. Never guess and resend.
    if prior.is_none() && job.recovery_count > 0 {
        return Err(JobFailureKind::DeliveryUncertain);
    }
    let mut scan = Lifecycle::open(&task.rollout_path)?;
    if scan.session_id.as_deref() != Some(&job.task_id) {
        return Err(JobFailureKind::InvalidOutput);
    }
    // Unresolved older deliveries fence this conversation, including jobs marked failed after timeout.
    let mut statement = db
        .prepare(
            "SELECT request_id, state, turn_id FROM deliveries
        WHERE task_id=?1 AND state IN ('intent','accepted') AND request_id!=?2",
        )
        .map_err(|_| JobFailureKind::ProcessIo)?;
    let older = statement
        .query_map(params![job.task_id, job.request_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|_| JobFailureKind::ProcessIo)?;
    for row in older {
        let (id, state, turn) = row.map_err(|_| JobFailureKind::ProcessIo)?;
        if state == "intent" {
            return Err(JobFailureKind::DeliveryUncertain);
        }
        let turn = turn.ok_or(JobFailureKind::InvalidOutput)?;
        let outcome = scan.wait_turn(&turn, cancel, deadline);
        if matches!(outcome, Ok(()) | Err(JobFailureKind::ExitFailure)) {
            finish(&db, &id, outcome.is_ok())?;
        } else {
            return outcome;
        }
    }
    let turn = if let Some((_, Some(turn))) = prior {
        turn
    } else {
        scan.wait_idle(cancel, deadline)?;
        let mut ipc = Ipc::connect(cancel, deadline)?;
        let owner = ipc.request(
            "thread-owner-discovery",
            1,
            json!({"hostId":"local","conversationId":job.task_id}),
            None,
            cancel,
            deadline,
        )?;
        let target = owner["handledByClientId"]
            .as_str()
            .ok_or(JobFailureKind::ActiveSession)?;
        scan.wait_idle(cancel, deadline)?;
        db.execute(
            "INSERT INTO deliveries(request_id,task_id,state) VALUES(?1,?2,'intent')",
            params![job.request_id, job.task_id],
        )
        .map_err(|_| JobFailureKind::ProcessIo)?;
        let reply = ipc
            .request(
                "thread-follower-start-turn",
                2,
                start_payload(job),
                Some(target),
                cancel,
                deadline,
            )
            .map_err(|_| JobFailureKind::DeliveryUncertain)?;
        let turn = reply
            .pointer("/result/result/turn/id")
            .and_then(Value::as_str)
            .ok_or(JobFailureKind::InvalidOutput)?
            .to_owned();
        db.execute(
            "UPDATE deliveries SET state='accepted',turn_id=?2 WHERE request_id=?1",
            params![job.request_id, turn],
        )
        .map_err(|_| JobFailureKind::ProcessIo)?;
        turn
    };
    let outcome = scan.wait_turn(&turn, cancel, deadline);
    if matches!(outcome, Ok(()) | Err(JobFailureKind::ExitFailure)) {
        finish(&db, &job.request_id, outcome.is_ok())?;
    }
    outcome
}

fn finish(db: &Connection, id: &str, success: bool) -> Result<()> {
    db.execute(
        "UPDATE deliveries SET state=?2 WHERE request_id=?1",
        params![id, if success { "completed" } else { "aborted" }],
    )
    .map_err(|_| JobFailureKind::ProcessIo)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(&path, b"").unwrap();
        (dir, path)
    }

    fn append(path: &Path, value: Value) {
        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        writeln!(file, "{value}").unwrap();
    }

    fn event(kind: &str, turn: &str) -> Value {
        json!({"type":"event_msg","payload":{"type":kind,"turn_id":turn,"last_agent_message":"done"}})
    }

    #[test]
    fn only_matching_completion_releases_turn_and_partial_lines_wait() {
        let (_dir, path) = fixture();
        append(&path, event("task_started", "wanted"));
        let mut scan = Lifecycle::open(&path).unwrap();
        append(&path, event("task_complete", "unrelated"));
        scan.poll().unwrap();
        assert_eq!(scan.active.as_deref(), Some("wanted"));
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        let raw = serde_json::to_vec(&event("task_complete", "wanted")).unwrap();
        file.write_all(&raw[..raw.len() / 2]).unwrap();
        scan.poll().unwrap();
        assert_eq!(scan.active.as_deref(), Some("wanted"));
        file.write_all(&raw[raw.len() / 2..]).unwrap();
        scan.poll().unwrap();
        assert!(!scan.terminal.contains_key("wanted"));
        file.write_all(b"\n").unwrap();
        scan.wait_turn(
            "wanted",
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
        assert!(scan.active.is_none());
    }

    #[test]
    fn abort_is_failure_and_timeout_is_not_completion() {
        let (_dir, path) = fixture();
        append(&path, event("task_started", "a"));
        append(&path, event("turn_aborted", "a"));
        let mut scan = Lifecycle::open(&path).unwrap();
        assert_eq!(
            scan.wait_turn(
                "a",
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(1)
            ),
            Err(JobFailureKind::ExitFailure)
        );
        assert_eq!(
            scan.wait_turn("missing", &AtomicBool::new(false), Instant::now()),
            Err(JobFailureKind::Timeout)
        );
        assert_eq!(
            scan.wait_idle(
                &AtomicBool::new(true),
                Instant::now() + Duration::from_secs(1)
            ),
            Err(JobFailureKind::ProcessIo)
        );
        append(
            &path,
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"error",
            "error":{"codex_error_info":"server_overloaded"},"last_agent_message":null}}),
        );
        assert_eq!(
            scan.wait_turn(
                "error",
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(1)
            ),
            Err(JobFailureKind::ExitFailure)
        );
    }

    #[test]
    fn malformed_oversized_and_truncated_rollouts_fail_closed() {
        let (_dir, path) = fixture();
        std::fs::write(&path, b"not json\n").unwrap();
        assert!(matches!(
            Lifecycle::open(&path),
            Err(JobFailureKind::InvalidOutput)
        ));
        std::fs::write(&path, vec![b'x'; MAX_LINE + 1]).unwrap();
        assert!(matches!(
            Lifecycle::open(&path),
            Err(JobFailureKind::OutputTooLarge)
        ));
        std::fs::write(&path, b"{}\n").unwrap();
        let mut scan = Lifecycle::open(&path).unwrap();
        std::fs::write(&path, b"").unwrap();
        assert_eq!(scan.poll(), Err(JobFailureKind::InvalidOutput));
    }

    #[test]
    fn journal_preserves_ambiguous_intent_and_accepted_turn_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("delivery.sqlite3");
        let db = journal(&path).unwrap();
        db.execute(
            "INSERT INTO deliveries VALUES('a','thread',NULL,'intent')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO deliveries VALUES('b','thread','turn-b','accepted')",
            [],
        )
        .unwrap();
        drop(db);
        let db = journal(&path).unwrap();
        let intent: String = db
            .query_row(
                "SELECT state FROM deliveries WHERE request_id='a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(intent, "intent");
        let turn: String = db
            .query_row(
                "SELECT turn_id FROM deliveries WHERE request_id='b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(turn, "turn-b");
        finish(&db, "b", false).unwrap();
        let state: String = db
            .query_row(
                "SELECT state FROM deliveries WHERE request_id='b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "aborted");
    }

    #[test]
    fn payload_contains_renderer_required_text_elements_and_keeps_settings() {
        let job = Job {
            request_id: Uuid::new_v4().to_string(),
            task_id: Uuid::new_v4().to_string(),
            slot: 1,
            generation: 1,
            prompt: "语音测试".into(),
            cwd: "C:\\test".into(),
            recovery_count: 0,
            claim_generation: 1,
        };
        let payload = start_payload(&job);
        assert_eq!(
            payload["turnStart"]["request"]["input"][0]["text_elements"],
            json!([])
        );
        assert_eq!(payload["turnStart"]["request"]["threadId"], job.task_id);
        assert_eq!(
            payload["turnStart"]["context"]["inheritThreadSettings"],
            true
        );
    }
}
