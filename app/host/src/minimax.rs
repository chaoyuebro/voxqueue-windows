//! MiniMax voice API adapter. No key, transcript, prompt or audio is logged.

use std::io::Read;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::blocking::multipart::{Form, Part};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::{Value, json};
use thiserror::Error;

use crate::audio::{MAX_TTS_SECONDS, TTS_SAMPLE_RATE};
use crate::dashscope::{TtsAudio, TtsReceipt};

pub const ASR_MODEL: &str = "asr-1.0";
pub const TTS_MODEL: &str = "speech-2.8-hd";
pub const TTS_VOICE: &str = "Chinese (Mandarin)_Lyrical_Voice";
const ASR_URL: &str = "https://api.minimax.cn/v1/speech_to_text";
const TTS_URL: &str = "https://api.minimax.cn/v1/t2a_v2";
const TTS_SOURCE_RATE: u32 = 44_100;
const MAX_ASR_RESPONSE: u64 = 64 * 1024;
const MAX_TTS_RESPONSE: u64 = 32 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum VoiceError {
    #[error("voice request is invalid")]
    InvalidRequest,
    #[error("MiniMax rejected the credential")]
    Rejected,
    #[error("MiniMax rate limited the request")]
    RateLimited,
    #[error("MiniMax is unavailable")]
    Unavailable,
    #[error("MiniMax response is invalid")]
    Protocol,
    #[error("audio exceeds the playback limit")]
    AudioLimit,
    #[error("TTS submission may have been accepted; automatic retry is unsafe")]
    AmbiguousAfterCommit,
}

pub struct VoiceClient {
    http: Client,
}

impl VoiceClient {
    pub fn new() -> Result<Self, VoiceError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| VoiceError::Unavailable)?;
        Ok(Self { http })
    }

    pub fn transcribe_wav(&self, key: &[u8], wav: &[u8]) -> Result<String, VoiceError> {
        if wav.len() < 44 || wav.len() > 16_000 * 2 * 90 + 44 || !wav.starts_with(b"RIFF") {
            return Err(VoiceError::InvalidRequest);
        }
        let bearer = bearer(key)?;
        let file = Part::bytes(wav.to_vec())
            .file_name("capture.wav")
            .mime_str("audio/wav")
            .map_err(|_| VoiceError::InvalidRequest)?;
        let form = Form::new()
            .text("model", ASR_MODEL)
            .text("response_format", "json")
            .text("stream", "false")
            .part("file", file);
        let response = self
            .http
            .post(ASR_URL)
            .header(AUTHORIZATION, bearer)
            .multipart(form)
            .send()
            .map_err(|_| VoiceError::Unavailable)?;
        let body = read_response(response, MAX_ASR_RESPONSE)?;
        parse_asr(&body)
    }

    pub fn synthesize(&self, key: &[u8], text: &str) -> Result<TtsAudio, VoiceError> {
        self.synthesize_with_settings(key, text, &crate::voice_settings::VoiceSettings::default())
    }

    pub fn synthesize_with_settings(&self, key: &[u8], text: &str, settings: &crate::voice_settings::VoiceSettings) -> Result<TtsAudio, VoiceError> {
        if text.is_empty() || text.chars().count() >= 10_000 || text.chars().any(char::is_control) {
            return Err(VoiceError::InvalidRequest);
        }
        let bearer = bearer(key)?;
        let request = synthesis_request(text, settings)?;
        let response = self
            .http
            .post(TTS_URL)
            .header(AUTHORIZATION, bearer)
            .header(CONTENT_TYPE, "application/json")
            .json(&request)
            .send()
            .map_err(|_| VoiceError::AmbiguousAfterCommit)?;
        let body = read_response(response, MAX_TTS_RESPONSE).map_err(|error| match error {
            VoiceError::Rejected | VoiceError::RateLimited => error,
            _ => VoiceError::AmbiguousAfterCommit,
        })?;
        let (source, characters) = parse_tts(&body)?;
        let pcm = resample_pcm16_mono(&source)?;
        // Keyboard hardware volume is the sole playback volume control.
        let samples = (pcm.len() / 2) as u64;
        Ok(TtsAudio::from_provider(
            pcm,
            TtsReceipt {
                model: TTS_MODEL,
                voice: settings.voice.clone(),
                sample_rate: TTS_SAMPLE_RATE,
                samples,
                characters,
                transport: "minimax-http",
                attempts: 1,
            },
        ))
    }
}

#[doc(hidden)]
pub fn synthesis_request(text: &str, settings: &crate::voice_settings::VoiceSettings) -> Result<Value, VoiceError> {
    settings.validate().map_err(|_| VoiceError::InvalidRequest)?;
    Ok(json!({
        "model": TTS_MODEL, "text": text, "stream": false, "output_format": "hex",
        "voice_setting": {"voice_id": settings.voice, "speed": settings.speed, "vol": 1.0, "pitch": 0},
        "audio_setting": {"sample_rate": TTS_SOURCE_RATE, "format": "pcm", "channel": 1}
    }))
}

/// Scale PCM locally so 0% is exact silence and the original 100% level stays unchanged.
#[doc(hidden)]
pub fn apply_volume(pcm: &mut [u8], volume: f64) -> Result<(), VoiceError> {
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) || pcm.len() % 2 != 0 {
        return Err(VoiceError::InvalidRequest);
    }
    for bytes in pcm.chunks_exact_mut(2) {
        let sample = i16::from_le_bytes([bytes[0], bytes[1]]);
        bytes.copy_from_slice(&((f64::from(sample) * volume).round() as i16).to_le_bytes());
    }
    Ok(())
}

fn bearer(key: &[u8]) -> Result<String, VoiceError> {
    if key.len() < 20
        || key.len() > 4096
        || key
            .iter()
            .any(|byte| byte.is_ascii_whitespace() || !byte.is_ascii_graphic())
    {
        return Err(VoiceError::InvalidRequest);
    }
    let key = std::str::from_utf8(key).map_err(|_| VoiceError::InvalidRequest)?;
    Ok(format!("Bearer {key}"))
}

fn read_response(response: reqwest::blocking::Response, limit: u64) -> Result<Vec<u8>, VoiceError> {
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(VoiceError::Rejected);
    }
    if status.as_u16() == 429 {
        return Err(VoiceError::RateLimited);
    }
    if !status.is_success() {
        return Err(VoiceError::Unavailable);
    }
    if response.content_length().is_some_and(|size| size > limit) {
        return Err(VoiceError::Protocol);
    }
    let mut body = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut body)
        .map_err(|_| VoiceError::Unavailable)?;
    if body.len() as u64 > limit {
        return Err(VoiceError::Protocol);
    }
    Ok(body)
}

#[doc(hidden)]
pub fn parse_asr(body: &[u8]) -> Result<String, VoiceError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| VoiceError::Protocol)?;
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .ok_or(VoiceError::Protocol)?
        .trim();
    if text.is_empty() || text.len() > 16 * 1024 {
        return Err(VoiceError::Protocol);
    }
    let duration = value
        .get("duration")
        .and_then(Value::as_f64)
        .ok_or(VoiceError::Protocol)?;
    if !duration.is_finite() || duration <= 0.0 || duration > 90.5 {
        return Err(VoiceError::Protocol);
    }
    Ok(text.to_owned())
}

#[doc(hidden)]
pub fn parse_tts(body: &[u8]) -> Result<(Vec<u8>, Option<u64>), VoiceError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| VoiceError::Protocol)?;
    if value
        .pointer("/base_resp/status_code")
        .and_then(Value::as_i64)
        != Some(0)
        || value.pointer("/data/status").and_then(Value::as_u64) != Some(2)
        || value
            .pointer("/extra_info/audio_sample_rate")
            .and_then(Value::as_u64)
            != Some(u64::from(TTS_SOURCE_RATE))
        || value
            .pointer("/extra_info/audio_channel")
            .and_then(Value::as_u64)
            != Some(1)
        || value
            .pointer("/extra_info/audio_format")
            .and_then(Value::as_str)
            != Some("pcm")
    {
        return Err(VoiceError::Protocol);
    }
    let encoded = value
        .pointer("/data/audio")
        .and_then(Value::as_str)
        .ok_or(VoiceError::Protocol)?;
    if encoded.is_empty()
        || encoded.len() % 4 != 0
        || encoded.len() > (TTS_SOURCE_RATE as usize * MAX_TTS_SECONDS as usize * 4)
    {
        return Err(VoiceError::AudioLimit);
    }
    let source = decode_hex(encoded.as_bytes())?;
    if source.len() % 2 != 0
        || source.is_empty()
        || value
            .pointer("/extra_info/audio_size")
            .and_then(Value::as_u64)
            != Some(source.len() as u64)
    {
        return Err(VoiceError::Protocol);
    }
    let characters = value
        .pointer("/extra_info/usage_characters")
        .and_then(Value::as_u64);
    Ok((source, characters))
}

fn decode_hex(encoded: &[u8]) -> Result<Vec<u8>, VoiceError> {
    fn digit(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let mut output = Vec::with_capacity(encoded.len() / 2);
    for pair in encoded.chunks_exact(2) {
        let high = digit(pair[0]).ok_or(VoiceError::Protocol)?;
        let low = digit(pair[1]).ok_or(VoiceError::Protocol)?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

#[doc(hidden)]
pub fn resample_pcm16_mono(source: &[u8]) -> Result<Vec<u8>, VoiceError> {
    let input: Vec<i16> = source
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if input.is_empty() || input.len() > TTS_SOURCE_RATE as usize * MAX_TTS_SECONDS as usize {
        return Err(VoiceError::AudioLimit);
    }
    let output_samples = input.len() * TTS_SAMPLE_RATE as usize / TTS_SOURCE_RATE as usize;
    let mut output = Vec::with_capacity(output_samples * 2);
    for index in 0..output_samples {
        let position = index as u64 * u64::from(TTS_SOURCE_RATE);
        let left = (position / u64::from(TTS_SAMPLE_RATE)) as usize;
        let fraction = (position % u64::from(TTS_SAMPLE_RATE)) as i64;
        let a = i64::from(input[left]);
        let b = i64::from(input[(left + 1).min(input.len() - 1)]);
        let sample = a + (b - a) * fraction / i64::from(TTS_SAMPLE_RATE);
        output.extend_from_slice(&(sample as i16).to_le_bytes());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn parses_asr_without_exposing_other_fields() {
        assert_eq!(
            parse_asr(r#"{"text":"你好","duration":1.2,"trace_id":"opaque"}"#.as_bytes()).unwrap(),
            "你好"
        );
        assert!(parse_asr(br#"{"text":"","duration":1.2}"#).is_err());
    }

    #[test]
    fn parses_pcm_and_converts_to_device_rate() {
        let body = br#"{"data":{"audio":"0000010002000300","status":2},"extra_info":{"audio_sample_rate":44100,"audio_channel":1,"audio_format":"pcm","audio_size":8,"usage_characters":2},"base_resp":{"status_code":0}}"#;
        let (source, characters) = parse_tts(body).unwrap();
        assert_eq!(characters, Some(2));
        assert_eq!(source.len(), 8);
        let output = resample_pcm16_mono(&source).unwrap();
        assert_eq!(output.len(), 8);
    }
}
