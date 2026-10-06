use easy_codex_host::{voice_activity::VoiceActivityTracker, store::{StateStore,NewJob}};
use std::time::{Instant,Duration};
#[test]
fn late_asr_cannot_replace_new_recording_and_clear_preserves_live_capture() {
 let tracker=VoiceActivityTracker::default();
 tracker.recording(1,1); tracker.transition(1,1,"recognizing");
 tracker.recording(1,2); tracker.transition(1,1,"recognition_failed");
 assert_eq!(tracker.snapshot()[&1],"recording");
 tracker.dismiss_failed(1); assert_eq!(tracker.snapshot()[&1],"recording");
 tracker.transition(1,2,"recognizing"); tracker.expire_recordings(&[]);
 assert_eq!(tracker.snapshot()[&1],"recognizing");
 tracker.transition(1,2,"waiting_delivery"); tracker.transition(1,2,"idle");
 tracker.recording(2,3); tracker.expire_recordings(&[]);
 assert_eq!(tracker.snapshot()[&2],"recording_failed");
 tracker.dismiss_failed(2); assert!(!tracker.snapshot().contains_key(&2));
 assert!(tracker.snapshot_at(Instant::now()+Duration::from_secs(301)).is_empty());
}
#[test]
fn active_job_tracks_exact_slot_binding_and_running_before_queued() {
 let dir=tempfile::tempdir().unwrap(); let mut store=StateStore::open(&dir.path().join("db.sqlite3")).unwrap();
 store.set_binding(1,None,"task").unwrap();
 for id in ["a","b"] { store.enqueue(&NewJob{request_id:id,task_id:"task",slot:1,generation:1,prompt:"hello",cwd:dir.path()}).unwrap(); }
 assert_eq!(store.active_slot_job("task",1,1).unwrap().unwrap(),("a".into(),"queued".into()));
 store.claim_next_runnable().unwrap().unwrap();
 assert_eq!(store.active_slot_job("task",1,1).unwrap().unwrap(),("a".into(),"running".into()));
 assert!(store.active_slot_job("task",2,1).unwrap().is_none());
 assert!(store.active_slot_job("task",1,2).unwrap().is_none());
}

#[test]
fn authenticated_audio_marks_recording_and_short_capture_reports_failure_without_asr() {
 use easy_codex_host::{lan_voice::{LanVoiceConfig,LanVoiceIngress},paths::AppPaths};
 use std::net::UdpSocket; use hmac::{Hmac,Mac}; use sha2::Sha256;
 let dir=tempfile::tempdir().unwrap();let key=[7;32];
 let mut config=LanVoiceConfig::from_paths(&AppPaths::from_root(dir.path().join("root")));config.bind_port=0;config.auth_key=Some(key);
 let ingress=LanVoiceIngress::start(config).unwrap();let socket=UdpSocket::bind("127.0.0.1:0").unwrap();let target=("127.0.0.1",ingress.local_port());
 let session= (0xECu64<<56)|(1u64<<52)|(1u64<<32)|1;
 let mut packet=vec![0u8;688];packet[..4].copy_from_slice(b"EIAU");packet[4]=3;packet[5]=32;packet[6]=1;packet[7]=1;
 packet[8..16].copy_from_slice(&session.to_le_bytes());packet[20..24].copy_from_slice(&16000u32.to_le_bytes());packet[28..30].copy_from_slice(&320u16.to_le_bytes());packet[30..32].copy_from_slice(&640u16.to_le_bytes());
 let mut mac=Hmac::<Sha256>::new_from_slice(&key).unwrap();mac.update(&packet[..672]);packet[672..].copy_from_slice(&mac.finalize().into_bytes()[..16]);
 let mut bad=packet.clone();bad[9]^=1;socket.send_to(&bad,target).unwrap();std::thread::sleep(Duration::from_millis(70));assert!(ingress.diagnostics().slot_activity.is_empty());
 socket.send_to(&packet,target).unwrap();wait_phase(&ingress,"recording");
 let mut end=vec![0u8;48];end[..4].copy_from_slice(b"EIAE");end[4]=3;end[5]=32;end[6]=1;end[8..16].copy_from_slice(&session.to_le_bytes());end[16..20].copy_from_slice(&1u32.to_le_bytes());end[20..24].copy_from_slice(&16000u32.to_le_bytes());
 let mut mac=Hmac::<Sha256>::new_from_slice(&key).unwrap();mac.update(&end[..32]);end[32..].copy_from_slice(&mac.finalize().into_bytes()[..16]);socket.send_to(&end,target).unwrap();wait_phase(&ingress,"recording_failed");
 assert_eq!(ingress.diagnostics().asr_succeeded,0);assert_eq!(ingress.diagnostics().captures_ready,0);
 ingress.dismiss_failed_activity(1);assert!(ingress.diagnostics().slot_activity.is_empty());
}
fn wait_phase(ingress:&easy_codex_host::lan_voice::LanVoiceIngress,phase:&str){
 let deadline=Instant::now()+Duration::from_secs(2);
 while ingress.diagnostics().slot_activity.get(&1).map(String::as_str)!=Some(phase)&&Instant::now()<deadline{std::thread::sleep(Duration::from_millis(5));}
 assert_eq!(ingress.diagnostics().slot_activity.get(&1).map(String::as_str),Some(phase));
}
