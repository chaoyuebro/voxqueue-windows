//! Ephemeral capture progress, guarded by session identity to reject late ASR updates.
use std::{collections::BTreeMap, sync::Mutex, time::{Duration, Instant}};
#[derive(Default)]
pub struct VoiceActivityTracker { slots: Mutex<BTreeMap<u8, (u64, String, Instant)>> }
impl VoiceActivityTracker {
    pub fn recording(&self, slot: u8, session: u64) {
        if let Ok(mut slots) = self.slots.lock() {
            let entry = slots.entry(slot).or_insert((session, "recording".into(), Instant::now()));
            if entry.0 != session { *entry = (session, "recording".into(), Instant::now()); }
            else if entry.1 == "recording" { entry.2 = Instant::now(); }
        }
    }
    pub fn transition(&self, slot: u8, session: u64, phase: &str) {
        if let Ok(mut slots) = self.slots.lock() {
            if let Some(entry) = slots.get_mut(&slot).filter(|entry| entry.0 == session) {
                *entry = (session, phase.into(), Instant::now());
            }
        }
    }
    pub fn snapshot(&self) -> BTreeMap<u8, String> { self.snapshot_at(Instant::now()) }
    pub fn snapshot_at(&self, now: Instant) -> BTreeMap<u8, String> {
        self.slots.lock().map(|slots| slots.iter().filter(|(_, (_, _, at))| now.saturating_duration_since(*at) < Duration::from_secs(300)).map(|(slot, (_, phase, _))| (*slot, phase.clone())).collect()).unwrap_or_default()
    }
    pub fn expire_recordings(&self, active: &[u64]) {
        if let Ok(mut slots) = self.slots.lock() {
            for (_, (session, phase, at)) in slots.iter_mut() {
                if phase == "recording" && !active.contains(session) { *phase = "recording_failed".into(); *at = Instant::now(); }
            }
        }
    }
    pub fn dismiss_failed(&self, slot: u8) {
        if let Ok(mut slots) = self.slots.lock() {
            if slots.get(&slot).is_some_and(|(_, phase, _)| phase.ends_with("_failed")) { slots.remove(&slot); }
        }
    }
}
