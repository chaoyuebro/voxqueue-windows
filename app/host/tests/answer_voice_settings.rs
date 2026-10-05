use easy_codex_host::voice_settings::{VoiceSettings, voices};
use easy_codex_host::minimax::synthesis_request;

#[test]
fn saved_preferences_survive_reopen_and_reach_provider_request() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(VoiceSettings::load(root.path()).unwrap(), VoiceSettings::default());
    for voice in voices() {
        for speed in [0.5, 1.0, 1.35, 2.0] {
            let settings = VoiceSettings { voice: voice.id.into(), speed, volume: 0.65 };
            settings.save(root.path()).unwrap();
            let reopened = VoiceSettings::load(root.path()).unwrap();
            assert_eq!(reopened, settings);
            let request = synthesis_request("测试播报", &reopened).unwrap();
            assert_eq!(request["voice_setting"]["voice_id"], settings.voice);
            assert_eq!(request["voice_setting"]["speed"], speed);
            assert_eq!(request["audio_setting"]["sample_rate"], 44100);
        }
    }
}

#[test]
fn invalid_changes_leave_saved_preferences_intact() {
    let root = tempfile::tempdir().unwrap();
    let valid = VoiceSettings::default();
    valid.save(root.path()).unwrap();
    for speed in [0.49, 2.01, f64::NAN, f64::INFINITY] {
        let invalid = VoiceSettings { speed, ..valid.clone() };
        assert!(invalid.save(root.path()).is_err());
        assert!(synthesis_request("测试", &invalid).is_err());
        assert_eq!(VoiceSettings::load(root.path()).unwrap(), valid);
    }
    let invalid = VoiceSettings { voice: "unknown".into(), ..valid.clone() };
    assert!(invalid.save(root.path()).is_err());
    assert_eq!(VoiceSettings::load(root.path()).unwrap(), valid);
    std::fs::write(root.path().join("voice-settings.json"), "{broken").unwrap();
    assert!(VoiceSettings::load(root.path()).is_err());
}

#[cfg(windows)]
#[test]
fn orchestrator_uses_selected_voice_as_receipt_expectation() {
    use easy_codex_host::summary_orchestrator::{MiniMaxSummarySynthesizer, SummarySynthesizer};
    let client = easy_codex_host::minimax::VoiceClient::new().unwrap();
    let synthesizer = MiniMaxSummarySynthesizer {
        client: &client,
        key: b"test-not-a-real-key",
        settings: VoiceSettings { voice: voices()[1].id.into(), speed: 1.5, ..VoiceSettings::default() },
    };
    assert_eq!(synthesizer.voice(), voices()[1].id);
}

#[cfg(windows)]
#[test]
#[ignore = "Calls MiniMax using the current user's stored credential"]
fn live_selected_voices_and_speeds_produce_device_audio() {
    use easy_codex_host::{audio::encode_tts_audio, minimax::VoiceClient};
    let key = easy_codex_host::windows_credential::read_minimax_key().unwrap().expect("MiniMax credential required");
    let client = VoiceClient::new().unwrap();
    let text = "语音设置测试。之后的回答将使用您选择的音色和语速播报。";
    let mut durations = Vec::new();
    for (voice, speed) in [(voices()[1].id, 0.75), (voices()[1].id, 1.5), (voices()[4].id, 1.0)] {
        let settings = VoiceSettings { voice: voice.into(), speed, ..VoiceSettings::default() };
        let audio = client.synthesize_with_settings(&key, text, &settings).unwrap();
        assert_eq!(audio.receipt().voice, settings.voice);
        assert_eq!(audio.receipt().samples, (audio.pcm().len() / 2) as u64);
        let device = encode_tts_audio(audio.pcm()).unwrap();
        assert!(device.frames() > 0);
        durations.push(device.duration_ms());
        println!("voice={voice} speed={speed} duration_ms={} device_frames={}", device.duration_ms(), device.frames());
    }
    assert!(durations[0] > durations[1], "slower synthesis should produce longer audio for the same text/voice");
}

#[test]
fn volume_preserves_old_settings_and_scales_pcm_without_changing_duration() {
    use easy_codex_host::minimax::apply_volume;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("voice-settings.json"), r#"{"voice":"Chinese (Mandarin)_Sweet_Lady","speed":1.5}"#).unwrap();
    let old = VoiceSettings::load(root.path()).unwrap();
    assert_eq!(old.volume, 1.0);
    assert_eq!(old.speed, 1.5);
    let samples = [i16::MIN, -1000, 0, 1000, i16::MAX];
    let original: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
    for volume in [0.0, 0.25, 0.5, 1.0] {
        let settings = VoiceSettings { volume, ..old.clone() };
        settings.save(root.path()).unwrap();
        let saved = VoiceSettings::load(root.path()).unwrap();
        assert_eq!(saved, settings);
        let mut pcm = original.clone();
        apply_volume(&mut pcm, saved.volume).unwrap();
        assert_eq!(pcm.len(), original.len());
        for (bytes, sample) in pcm.chunks_exact(2).zip(samples) {
            assert_eq!(i16::from_le_bytes([bytes[0], bytes[1]]), (f64::from(sample) * volume).round() as i16);
        }
    }
    for volume in [-0.01, 1.01, f64::NAN, f64::INFINITY] {
        assert!(VoiceSettings { volume, ..old.clone() }.save(root.path()).is_err());
        assert!(apply_volume(&mut original.clone(), volume).is_err());
    }
}
