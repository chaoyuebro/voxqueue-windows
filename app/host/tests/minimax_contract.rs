use easy_codex_host::minimax::{parse_asr, parse_tts, resample_pcm16_mono};

#[test]
fn asr_requires_complete_bounded_result() {
    assert_eq!(
        parse_asr(r#"{"text":"你好","duration":1.2,"trace_id":"opaque"}"#.as_bytes()).unwrap(),
        "你好"
    );
    assert!(parse_asr(br#"{"text":"","duration":1.2}"#).is_err());
    assert!(parse_asr(br#"{"text":"hello","duration":999}"#).is_err());
}

#[test]
fn tts_requires_success_and_matching_pcm_metadata() {
    let valid = br#"{"data":{"audio":"0000010002000300","status":2},"extra_info":{"audio_sample_rate":44100,"audio_channel":1,"audio_format":"pcm","audio_size":8,"usage_characters":2},"base_resp":{"status_code":0}}"#;
    let (source, billed) = parse_tts(valid).unwrap();
    assert_eq!(billed, Some(2));
    assert_eq!(resample_pcm16_mono(&source).unwrap().len(), 8);
    let bad = br#"{"data":{"audio":"0000010002000300","status":2},"extra_info":{"audio_sample_rate":44100,"audio_channel":1,"audio_format":"pcm","audio_size":7},"base_resp":{"status_code":0}}"#;
    assert!(parse_tts(bad).is_err());
}
