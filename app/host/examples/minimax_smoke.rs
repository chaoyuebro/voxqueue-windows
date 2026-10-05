//! Exercise MiniMax TTS and ASR with a fixed, nonsensitive phrase.

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = easy_codex_host::windows_credential::read_minimax_key()?
        .ok_or("MiniMax credential is not configured")?;
    let client = easy_codex_host::minimax::VoiceClient::new()?;
    let tts = client.synthesize(&key, "你好，这是语音测试。")?;
    let source = tts.pcm();
    let pcm16: Vec<u8> = source.chunks_exact(6).flat_map(|chunk| chunk[..2].to_vec()).collect();
    let mut wav = Vec::with_capacity(44 + pcm16.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36u32 + pcm16.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&16_000u32.to_le_bytes());
    wav.extend_from_slice(&32_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm16.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm16);
    let transcript = client.transcribe_wav(&key, &wav)?;
    println!("tts=ok pcm_bytes={}", source.len());
    println!("asr=ok transcript_characters={}", transcript.chars().count());
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows credential store is required");
}
