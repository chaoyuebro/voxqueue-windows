//! Codex desktop version-11 state notifications. No rollout files are opened.
use crate::desktop_runner::Ipc;
use crate::rollout_observer::{ObserverTick, TurnPack, redact_sensitive_text};
use crate::store::{DesktopTurnObservation, JobFailureKind, ObserverStateStore, StoreError};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, JobFailureKind>;
const TEXT_LIMIT: usize = 12 * 1024;

fn text(value: &Value) -> Value {
    let Some(raw) = value.as_str() else {
        return Value::Null;
    };
    let mut end = raw.len().min(TEXT_LIMIT);
    while !raw.is_char_boundary(end) {
        end -= 1;
    }
    Value::String(raw[..end].to_owned())
}

// Preserve array positions for Immer patches while discarding tools and images.
fn item(value: &Value) -> Value {
    match value["type"].as_str() {
        Some("agentMessage") => json!({"type":"agentMessage","id":value["id"],
            "text":text(&value["text"]),"phase":value["phase"]}),
        Some("userMessage") => json!({"type":"userMessage","id":value["id"],
            "content":input(&value["content"])}),
        _ => Value::Null,
    }
}

fn input(value: &Value) -> Value {
    Value::Array(
        value
            .as_array()
            .into_iter()
            .flatten()
            .map(|part| {
                if matches!(
                    part["type"].as_str(),
                    Some("text" | "input_text" | "output_text")
                ) {
                    json!({"type":part["type"],"text":text(&part["text"])})
                } else {
                    Value::Null
                }
            })
            .collect(),
    )
}

fn turn(value: &Value) -> Value {
    json!({"turnId":value["turnId"],"status":value["status"],
        "turnStartedAtMs":value["turnStartedAtMs"],
        "error":!value["error"].is_null(),
        "params":{"input":input(&value["params"]["input"])},
        "items":value["items"].as_array().into_iter().flatten().map(item).collect::<Vec<_>>()})
}

fn project(path: &[String], value: &Value) -> Option<Value> {
    let mut tail = path;
    if tail.is_empty() {
        let mut root = serde_json::Map::new();
        for key in [
            "id",
            "resumeState",
            "threadRuntimeStatus",
            "turns",
            "turnHistory",
        ] {
            if let Some(value) = value.get(key).and_then(|v| project(&[key.into()], v)) {
                root.insert(key.into(), value);
            }
        }
        return Some(Value::Object(root));
    }
    match tail[0].as_str() {
        "id" | "resumeState" if tail.len() == 1 => return Some(text(value)),
        "threadRuntimeStatus" => {
            return match tail.len() {
                1 => Some(json!({"type":value["type"]})),
                2 if tail[1] == "type" => Some(text(value)),
                _ => None,
            };
        }
        "turns" => {
            if tail.len() == 1 {
                return Some(Value::Array(
                    value.as_array().into_iter().flatten().map(turn).collect(),
                ));
            }
            tail = &tail[2..];
        }
        "turnHistory" => {
            if tail.len() <= 3 {
                let mut map = serde_json::Map::new();
                let entities = match tail.len() {
                    1 => &value["history"]["entitiesByKey"],
                    2 if tail[1] == "history" => &value["entitiesByKey"],
                    3 if tail[1] == "history" && tail[2] == "entitiesByKey" => value,
                    _ => return None,
                };
                if let Some(entities) = entities.as_object() {
                    for (key, value) in entities {
                        map.insert(key.clone(), turn(value));
                    }
                }
                let entities = Value::Object(map);
                return Some(match tail.len() {
                    1 => json!({"history":{"entitiesByKey":entities}}),
                    2 => json!({"entitiesByKey":entities}),
                    _ => entities,
                });
            }
            if tail[1] != "history" || tail[2] != "entitiesByKey" {
                return None;
            }
            tail = &tail[4..];
        }
        _ => return None,
    }
    if tail.is_empty() {
        return Some(turn(value));
    }
    match tail[0].as_str() {
        "turnStartedAtMs" if tail.len() == 1 => Some(value.clone()),
        "turnId" | "status" if tail.len() == 1 => Some(text(value)),
        "error" if tail.len() == 1 => Some(json!(!value.is_null())),
        "params" => match tail.len() {
            1 => Some(json!({"input":input(&value["input"])})),
            2 if tail[1] == "input" => Some(input(value)),
            3 if tail[1] == "input" => Some(if value["type"] == "text" {
                json!({"type":"text","text":text(&value["text"])})
            } else {
                Value::Null
            }),
            4 if tail[1] == "input" && matches!(tail[3].as_str(), "type" | "text") => {
                Some(text(value))
            }
            _ => None,
        },
        "items" => match tail.len() {
            1 => Some(Value::Array(
                value.as_array().into_iter().flatten().map(item).collect(),
            )),
            2 => Some(item(value)),
            3 if matches!(tail[2].as_str(), "type" | "id" | "phase" | "text") => Some(text(value)),
            3 if tail[2] == "content" => Some(input(value)),
            4 if tail[2] == "content" => Some(if value["type"] == "text" {
                json!({"type":"text","text":text(&value["text"])})
            } else {
                Value::Null
            }),
            5 if tail[2] == "content" && matches!(tail[4].as_str(), "type" | "text") => {
                Some(text(value))
            }
            _ => None,
        },
        _ => None,
    }
}

fn patch(root: &mut Value, path: &[String], op: &str, value: Value) -> Result<()> {
    if path.is_empty() {
        *root = value;
        return Ok(());
    }
    let mut node = root;
    for key in &path[..path.len() - 1] {
        node = match node {
            Value::Object(map) => map.get_mut(key).ok_or(JobFailureKind::InvalidOutput)?,
            Value::Array(array) => array
                .get_mut(
                    key.parse::<usize>()
                        .map_err(|_| JobFailureKind::InvalidOutput)?,
                )
                .ok_or(JobFailureKind::InvalidOutput)?,
            // A tool or image placeholder: ignore its detail updates.
            Value::Null => return Ok(()),
            _ => return Err(JobFailureKind::InvalidOutput),
        };
    }
    let key = path.last().unwrap();
    match node {
        Value::Object(map) => {
            if op == "remove" {
                map.remove(key);
            } else {
                map.insert(key.clone(), value);
            }
        }
        Value::Array(array) => {
            if key == "length" {
                array.truncate(value.as_u64().ok_or(JobFailureKind::InvalidOutput)? as usize);
            } else {
                let index = key
                    .parse::<usize>()
                    .map_err(|_| JobFailureKind::InvalidOutput)?;
                if op == "remove" && index < array.len() {
                    array.remove(index);
                } else if op == "add" && index <= array.len() {
                    array.insert(index, value);
                } else if op == "replace" && index < array.len() {
                    array[index] = value;
                } else {
                    return Err(JobFailureKind::InvalidOutput);
                }
            }
        }
        Value::Null => {}
        _ => return Err(JobFailureKind::InvalidOutput),
    }
    Ok(())
}

pub struct StreamState {
    pub task: String,
    pub owner: String,
    pub revision: Option<u64>,
    state: Value,
}

impl StreamState {
    pub fn new(task: &str, owner: &str) -> Self {
        Self {
            task: task.into(),
            owner: owner.into(),
            revision: None,
            state: Value::Null,
        }
    }

    // Returns false on an unrelated notification; a revision gap requires a fresh snapshot.
    pub fn apply(&mut self, notification: &Value) -> Result<bool> {
        if notification["type"] != "broadcast"
            || notification["method"] != "thread-stream-state-changed"
            || notification["params"]["hostId"] != "local"
            || notification["params"]["conversationId"] != self.task
            || notification["sourceClientId"] != self.owner
        {
            return Ok(false);
        }
        if notification["version"] != 11 {
            self.revision = None;
            return Err(JobFailureKind::InvalidOutput);
        }
        let change = &notification["params"]["change"];
        let revision = change["revision"]
            .as_u64()
            .ok_or(JobFailureKind::InvalidOutput)?;
        if change["type"] == "snapshot" {
            if change["conversationState"]["id"] != self.task {
                return Err(JobFailureKind::InvalidOutput);
            }
            self.state =
                project(&[], &change["conversationState"]).ok_or(JobFailureKind::InvalidOutput)?;
        } else if change["type"] == "patches" {
            if self.revision.is_none() || change["baseRevision"].as_u64() != self.revision {
                self.revision = None;
                return Err(JobFailureKind::InvalidOutput);
            }
            let mut next = self.state.clone();
            for update in change["patches"]
                .as_array()
                .ok_or(JobFailureKind::InvalidOutput)?
            {
                let path = update["path"]
                    .as_array()
                    .ok_or(JobFailureKind::InvalidOutput)?
                    .iter()
                    .map(|key| {
                        key.as_str()
                            .map(str::to_owned)
                            .or_else(|| key.as_u64().map(|n| n.to_string()))
                            .ok_or(JobFailureKind::InvalidOutput)
                    })
                    .collect::<Result<Vec<_>>>()?;
                let Some(value) = project(&path, &update["value"]) else {
                    continue;
                };
                let op = update["op"].as_str().ok_or(JobFailureKind::InvalidOutput)?;
                if !matches!(op, "add" | "remove" | "replace") {
                    return Err(JobFailureKind::InvalidOutput);
                }
                patch(&mut next, &path, op, value)?;
            }
            self.state = next;
        } else {
            return Err(JobFailureKind::InvalidOutput);
        }
        self.revision = Some(revision);
        Ok(true)
    }

    pub fn turns(&self) -> Vec<&Value> {
        self.state["turns"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(
                self.state["turnHistory"]["history"]["entitiesByKey"]
                    .as_object()
                    .into_iter()
                    .flat_map(|m| m.values()),
            )
            .collect()
    }

    pub fn idle(&self) -> bool {
        self.revision.is_some()
            && self.state["resumeState"] == "resumed"
            && self.state["threadRuntimeStatus"]["type"] == "idle"
            && !self
                .turns()
                .iter()
                .any(|turn| turn["status"] == "inProgress")
    }

    pub fn outcome(&self, turn_id: &str) -> Option<bool> {
        if self.revision.is_none() {
            return None;
        }
        let turn = self
            .turns()
            .into_iter()
            .find(|turn| turn["turnId"] == turn_id)?;
        match turn["status"].as_str()? {
            "completed" if turn["error"] != false => Some(false),
            "completed" if final_answer(turn).is_some() => Some(true),
            "failed" | "interrupted" => Some(false),
            _ => None,
        }
    }

    pub(crate) fn matching_delivery(&self, baseline: &[String], prompt: &str, sent_at: u64) -> Option<String> {
        if self.revision.is_none() { return None; }
        let mut candidates = BTreeSet::new();
        for turn in self.turns() {
            let Some(id) = turn["turnId"].as_str() else { continue; };
            if baseline.iter().any(|old| old == id) || uuid::Uuid::parse_str(id).is_err() { continue; }
            let Some(started) = turn["turnStartedAtMs"].as_u64() else { continue; };
            if started < sent_at || started > sent_at.saturating_add(45_000) { continue; }
            let inputs = turn["params"]["input"].as_array();
            let exact = inputs.is_some_and(|inputs| inputs.len() == 1 && inputs[0]["type"] == "text" && inputs[0]["text"].as_str() == Some(prompt));
            if exact { candidates.insert(id.to_owned()); }
        }
        if candidates.len() == 1 { candidates.into_iter().next() } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lost_ack_matches_only_one_new_exact_input_in_send_window() {
        let id = "01a110b4-b3e6-76d2-9163-2befa6a92636";
        let other = "01a110b6-44c5-7751-b8e8-d9ad724a5770";
        let mut state = StreamState::new(TASK, "owner");
        let candidate = json!({"turnId":id,"turnStartedAtMs":1001,"status":"completed","error":null,
            "params":{"input":[{"type":"text","text":"test"}]},"items":[]});
        state.apply(&snapshot(candidate.clone())).unwrap();
        assert_eq!(state.matching_delivery(&[], "test", 1000).as_deref(), Some(id));
        assert_eq!(state.matching_delivery(&[id.to_owned()], "test", 1000), None);
        assert_eq!(state.matching_delivery(&[], "different", 1000), None);
        assert_eq!(state.matching_delivery(&[], "test", 1002), None);
        let mut second = candidate.clone(); second["turnId"] = json!(other);
        let mut ambiguous = snapshot(candidate);
        ambiguous["params"]["change"]["conversationState"]["turns"] = json!([second]);
        state.apply(&ambiguous).unwrap();
        assert_eq!(state.matching_delivery(&[], "test", 1000), None);
    }
    #[test]
    #[ignore = "requires explicit read-only target, input and send time"]
    fn live_read_only_delivery_confirmation() {
        let task = std::env::var("VOXQUEUE_CONFIRM_TASK").unwrap();
        let prompt = std::env::var("VOXQUEUE_CONFIRM_INPUT").unwrap();
        let sent_at: u64 = std::env::var("VOXQUEUE_CONFIRM_TIME").unwrap().parse().unwrap();
        let expected = std::env::var("VOXQUEUE_CONFIRM_TURN").unwrap();
        let cancel = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut subscription = Subscription::connect(&task, &cancel, deadline).unwrap();
        while subscription.state.revision.is_none() { subscription.poll(&cancel, deadline).unwrap(); }
        assert_eq!(subscription.state.matching_delivery(&[], &prompt, sent_at).as_deref(), Some(expected.as_str()));
        assert_eq!(subscription.state.outcome(&expected), Some(true));
        println!("Existing completed turn verified through read-only notifications");
    }
    const TASK: &str = "01a10bd2-6774-7c40-ad12-238d9b5df18c";
    fn event(change: Value) -> Value {
        json!({"type":"broadcast","method":"thread-stream-state-changed","version":11,
            "sourceClientId":"owner","params":{"hostId":"local","conversationId":TASK,"change":change}})
    }
    fn snapshot(turn: Value) -> Value {
        event(json!({"type":"snapshot","revision":1,"conversationState":{
            "id":TASK,"resumeState":"resumed","threadRuntimeStatus":{"type":"active"},
            "turnHistory":{"history":{"entitiesByKey":{"tail":turn}}},"turns":[]}}))
    }
    #[test]
    fn matching_notification_revision_and_final_answer_drive_completion() {
        let mut state = StreamState::new(TASK, "owner");
        let snap = snapshot(json!({"turnId":"wanted","status":"inProgress","error":null,
            "params":{"input":[{"type":"text","text":"input"}]},"items":[
                {"type":"commandExecution","output":"x".repeat(2*1024*1024)},
                {"type":"agentMessage","text":"progress","phase":"commentary"}]}));
        state.apply(&snap).unwrap();
        assert!(!state.idle());
        assert_eq!(state.outcome("wanted"), None);
        assert!(serde_json::to_vec(&state.state).unwrap().len() < 1024);
        let done = event(
            json!({"type":"patches","baseRevision":1,"revision":2,"patches":[
            {"op":"replace","path":["turnHistory","history","entitiesByKey","tail","status"],"value":"completed"},
            {"op":"add","path":["turnHistory","history","entitiesByKey","tail","items",2],
                "value":{"type":"agentMessage","text":"answer","phase":"final_answer"}},
            {"op":"replace","path":["threadRuntimeStatus","type"],"value":"idle"}]}),
        );
        let mut wrong = done.clone();
        wrong["sourceClientId"] = json!("other");
        assert!(!state.apply(&wrong).unwrap());
        assert_eq!(state.outcome("wanted"), None);
        state.apply(&done).unwrap();
        assert_eq!(state.outcome("wanted"), Some(true));
        assert_eq!(state.outcome("unrelated"), None);
        assert!(state.idle());
        let gap = event(json!({"type":"patches","baseRevision":9,"revision":10,"patches":[]}));
        assert!(state.apply(&gap).is_err());
        assert_eq!(state.outcome("wanted"), None);
        assert!(!state.idle());
        state.apply(&snap).unwrap();
        assert_eq!(state.revision, Some(1));
    }
    #[test]
    fn failure_interrupt_and_missing_final_are_never_success() {
        for status in ["failed", "interrupted", "completed"] {
            let mut state = StreamState::new(TASK, "owner");
            state
                .apply(&snapshot(
                    json!({"turnId":"x","status":status,"error":null,"items":[]}),
                ))
                .unwrap();
            assert_ne!(state.outcome("x"), Some(true));
        }
        let mut state = StreamState::new(TASK, "owner");
        state
            .apply(&snapshot(
                json!({"turnId":"x","status":"completed","error":{"message":"failed"},
            "items":[{"type":"agentMessage","text":"partial","phase":"final_answer"}]}),
            ))
            .unwrap();
        assert_eq!(state.outcome("x"), Some(false));
    }
    #[test]
    #[ignore = "requires VOXQUEUE_NOTIFICATION_LIVE_TARGET; verifies runner and summary observation"]
    fn live_runner_and_summary_ledger_use_notifications() {
        use crate::store::{Job, StateStore};
        let task =
            std::env::var("VOXQUEUE_NOTIFICATION_LIVE_TARGET").expect("explicit target required");
        let catalog = crate::codex_catalog::CodexTaskCatalog::from_environment().unwrap();
        let target = catalog.bound_task(&task).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let mut store = StateStore::open(&temp.path().join("state.sqlite3")).unwrap();
        store.set_binding(1, None, &task).unwrap();
        let mut observer_store = store.open_observer_store().unwrap();
        let mut observer = DesktopObserver::new();
        let deadline = Instant::now() + Duration::from_secs(120);
        while observer
            .streams
            .get(&task)
            .is_none_or(|stream| stream.state.revision.is_none())
        {
            assert!(Instant::now() < deadline);
            observer.tick_due_worker(&mut observer_store).unwrap();
        }
        assert_eq!(store.pending_summary_completion_count(&task).unwrap(), 0);
        let request = uuid::Uuid::new_v4().to_string();
        let job = Job {
            request_id: request.clone(),
            task_id: task.clone(),
            slot: 1,
            generation: 1,
            prompt: "只回复 VOXQUEUE_NOTIFICATION_LEDGER_OK，不调用工具，不修改文件。".into(),
            cwd: target.cwd,
            recovery_count: 0,
            claim_generation: 1,
        };
        let journal = temp.path().join("delivery.sqlite3");
        let journal_run = journal.clone();
        let runner = std::thread::spawn(move || {
            crate::desktop_runner::run(
                &job,
                &AtomicBool::new(false),
                Duration::from_secs(90),
                &journal_run,
            )
        });
        while !runner.is_finished() || store.pending_summary_completion_count(&task).unwrap() == 0 {
            assert!(Instant::now() < deadline);
            observer.tick_due_worker(&mut observer_store).unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
        runner.join().unwrap().unwrap();
        let db = rusqlite::Connection::open(journal).unwrap();
        let (turn, state): (String, String) = db
            .query_row(
                "SELECT turn_id,state FROM deliveries WHERE request_id=?1",
                [request],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, "completed");
        let state_db = rusqlite::Connection::open(temp.path().join("state.sqlite3")).unwrap();
        let pack: String = state_db
            .query_row(
                "SELECT turn_pack FROM completion_ledger WHERE completion_id=?1",
                [&turn],
                |row| row.get(0),
            )
            .unwrap();
        let pack: TurnPack = serde_json::from_str(&pack).unwrap();
        assert!(pack.assistant[0].contains("VOXQUEUE_NOTIFICATION_LEDGER_OK"));
        assert!(pack.tools.is_empty());
        assert_eq!(store.pending_summary_completion_count(&task).unwrap(), 1);
        println!("runner and summary ledger confirmed through notifications for {turn}");
    }
    #[test]
    #[ignore = "requires VOXQUEUE_NOTIFICATION_LIVE_TARGET; sends one controlled test prompt"]
    fn live_notification_roundtrip_without_rollout_access() {
        let task = std::env::var("VOXQUEUE_NOTIFICATION_LIVE_TARGET")
            .expect("explicit test target required");
        let cancel = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut sub = Subscription::connect(&task, &cancel, deadline).unwrap();
        sub.wait_idle(&cancel, deadline).unwrap();
        let owner = sub.state.owner.clone();
        let request_id = uuid::Uuid::new_v4().to_string();
        let reply = sub.ipc.request("thread-follower-start-turn",2,json!({"conversationId":task,
            "turnStart":{"request":{"threadId":task,"clientUserMessageId":request_id,
            "input":[{"type":"text","text":"只回复 VOXQUEUE_NOTIFICATION_OK，不调用工具，不修改文件。","text_elements":[]}]},
            "context":{"inheritThreadSettings":true,"attachments":[],"commentAttachments":[]}}}),
            Some(&owner),&cancel,deadline).unwrap();
        let turn = reply
            .pointer("/result/result/turn/id")
            .and_then(Value::as_str)
            .unwrap();
        sub.wait_turn(turn, &cancel, deadline).unwrap();
        assert!(
            sub.state
                .turns()
                .into_iter()
                .find(|v| v["turnId"] == turn)
                .and_then(final_answer)
                .unwrap()
                .contains("VOXQUEUE_NOTIFICATION_OK")
        );
        println!("notification completion confirmed for turn {turn}");
    }
}

fn final_answer(turn: &Value) -> Option<&str> {
    turn["items"]
        .as_array()?
        .iter()
        .rev()
        .find(|item| item["type"] == "agentMessage" && item["phase"] == "final_answer")?["text"]
        .as_str()
        .filter(|text| !text.is_empty())
}

pub(crate) struct Subscription {
    pub ipc: Ipc,
    pub state: StreamState,
}

impl Subscription {
    pub(crate) fn connect(task: &str, cancel: &AtomicBool, deadline: Instant) -> Result<Self> {
        let mut ipc = Ipc::connect(cancel, deadline)?;
        let reply = ipc.request(
            "thread-owner-discovery",
            1,
            json!({"hostId":"local","conversationId":task}),
            None,
            cancel,
            deadline,
        )?;
        let owner = reply["handledByClientId"]
            .as_str()
            .ok_or(JobFailureKind::ActiveSession)?;
        ipc.follow(task, owner, true)?;
        Ok(Self {
            state: StreamState::new(task, owner),
            ipc,
        })
    }

    pub(crate) fn poll(&mut self, cancel: &AtomicBool, deadline: Instant) -> Result<()> {
        let value = self.ipc.event(cancel, deadline)?;
        if value["method"] == "ipc-connection-reset" {
            return Err(JobFailureKind::ProcessIo);
        }
        if value["method"] == "client-status-changed"
            && value["params"]["clientId"] == self.state.owner
            && value["params"]["status"] == "disconnected"
        {
            return Err(JobFailureKind::ProcessIo);
        }
        if self.state.apply(&value).is_err() {
            self.state.revision = None;
            self.ipc.follow(&self.state.task, &self.state.owner, true)?;
        }
        Ok(())
    }

    pub(crate) fn wait_idle(&mut self, cancel: &AtomicBool, deadline: Instant) -> Result<()> {
        while !self.state.idle() {
            self.poll(cancel, deadline)?;
        }
        Ok(())
    }

    pub(crate) fn wait_turn(
        &mut self,
        turn: &str,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> Result<()> {
        loop {
            if let Some(success) = self.state.outcome(turn) {
                return if success {
                    Ok(())
                } else {
                    Err(JobFailureKind::ExitFailure)
                };
            }
            self.poll(cancel, deadline)?;
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = self.ipc.follow(&self.state.task, &self.state.owner, false);
    }
}

pub struct DesktopObserver {
    streams: BTreeMap<String, Subscription>,
    retry_at: BTreeMap<String, Instant>,
    committed: BTreeMap<String, (u64, u64)>,
}

impl DesktopObserver {
    pub fn new() -> Self {
        Self {
            streams: BTreeMap::new(),
            retry_at: BTreeMap::new(),
            committed: BTreeMap::new(),
        }
    }
    pub(crate) fn tick_due_worker(
        &mut self,
        store: &mut ObserverStateStore,
    ) -> std::result::Result<ObserverTick, StoreError> {
        let bindings = store.bindings()?;
        let tasks: BTreeSet<_> = bindings.iter().map(|b| b.task_id.clone()).collect();
        self.streams.retain(|task, _| tasks.contains(task));
        self.retry_at.retain(|task, _| tasks.contains(task));
        self.committed.retain(|task, _| tasks.contains(task));
        let cancel = AtomicBool::new(false);
        let mut tick = ObserverTick::default();
        for task in tasks {
            if !self.streams.contains_key(&task)
                && self
                    .retry_at
                    .get(&task)
                    .is_none_or(|at| *at <= Instant::now())
            {
                self.retry_at
                    .insert(task.clone(), Instant::now() + Duration::from_secs(5));
                if let Ok(stream) =
                    Subscription::connect(&task, &cancel, Instant::now() + Duration::from_secs(2))
                {
                    self.committed.remove(&task);
                    self.streams.insert(task.clone(), stream);
                }
            }
            let Some(stream) = self.streams.get_mut(&task) else {
                tick.failed_tasks += 1;
                continue;
            };
            let mut failed = false;
            for _ in 0..32 {
                match stream.poll(&cancel, Instant::now() + Duration::from_millis(2)) {
                    Ok(()) => {}
                    Err(JobFailureKind::Timeout) => break,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                self.streams.remove(&task);
                tick.failed_tasks += 1;
                continue;
            }
            if stream.state.revision.is_none() {
                continue;
            }
            if !stream.state.idle() {
                tick.running_tasks += 1;
            }
            let binding = bindings.iter().find(|b| b.task_id == task).unwrap();
            let checkpoint = (stream.state.revision.unwrap(), binding.generation);
            if self.committed.get(&task) == Some(&checkpoint) {
                continue;
            }
            let observations: Vec<_> = stream
                .state
                .turns()
                .into_iter()
                .filter_map(|turn| {
                    let id = turn["turnId"].as_str()?;
                    if uuid::Uuid::parse_str(id).is_err() {
                        return None;
                    }
                    let pack = if stream.state.outcome(id) == Some(true) {
                        Some(TurnPack {
                            v: 1,
                            turn_id: id.into(),
                            user: turn["params"]["input"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|v| v["text"].as_str())
                                .take(1)
                                .map(redact_sensitive_text)
                                .collect(),
                            assistant: vec![redact_sensitive_text(final_answer(turn).unwrap())],
                            tools: vec![],
                        })
                    } else {
                        None
                    };
                    Some(DesktopTurnObservation {
                        turn_id: id.to_owned(),
                        status: turn["status"].as_str().unwrap_or("").to_owned(),
                        started_at_ms: turn["turnStartedAtMs"].as_u64().unwrap_or(0),
                        turn_pack: pack.map(|mut p| {
                            let encoded = serde_json::to_string(&p).expect("turn pack serializes");
                            if encoded.len() <= 64 * 1024 {
                                return encoded;
                            }
                            p.user.clear();
                            let mut end = p.assistant[0].len().min(8192);
                            while !p.assistant[0].is_char_boundary(end) {
                                end -= 1;
                            }
                            p.assistant[0].truncate(end);
                            serde_json::to_string(&p).expect("bounded turn pack serializes")
                        }),
                    })
                })
                .collect();
            match store.observe_desktop_snapshot(binding, &observations) {
                Ok(count) => {
                    tick.inserted += count;
                    self.committed.insert(task, checkpoint);
                }
                Err(StoreError::BindingChanged) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(tick)
    }
}
