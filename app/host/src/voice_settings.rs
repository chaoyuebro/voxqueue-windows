//! Shared desktop/Host preferences; replacing the file is atomic.
use std::{io::{self, Write}, path::Path};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceSettings {
    pub voice: String,
    pub speed: f64,
    #[serde(default = "default_volume")]
    pub volume: f64,
}

fn default_volume() -> f64 { 1.0 }

#[derive(Debug, Clone, Serialize)]
pub struct VoiceOption {
    pub id: &'static str,
    pub name: &'static str,
}

pub fn voices() -> Vec<VoiceOption> {
    [
        (crate::minimax::TTS_VOICE, "抒情男声（默认）"),
        ("Chinese (Mandarin)_Gentleman", "温润男声"),
        ("Chinese (Mandarin)_Male_Announcer", "播报男声"),
        ("Chinese (Mandarin)_Radio_Host", "电台男主播"),
        ("Chinese (Mandarin)_News_Anchor", "新闻女声"),
        ("Chinese (Mandarin)_Sweet_Lady", "甜美女声"),
        ("Chinese (Mandarin)_Warm_Bestie", "温暖闺蜜"),
        ("Chinese (Mandarin)_Gentle_Senior", "温柔学姐"),
    ].into_iter().map(|(id, name)| VoiceOption { id, name }).collect()
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self { voice: crate::minimax::TTS_VOICE.into(), speed: 1.0, volume: 1.0 }
    }
}

impl VoiceSettings {
    pub fn validate(&self) -> io::Result<()> {
        if !voices().iter().any(|v| v.id == self.voice)
            || !self.speed.is_finite() || !(0.5..=2.0).contains(&self.speed)
            || !self.volume.is_finite() || !(0.0..=1.0).contains(&self.volume) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "音色、语速或音量无效"));
        }
        Ok(())
    }

    pub fn load(root: &Path) -> io::Result<Self> {
        let bytes = match std::fs::read(root.join("voice-settings.json")) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e),
        };
        if bytes.len() > 4096 { return Err(io::Error::new(io::ErrorKind::InvalidData, "语音设置文件过大")); }
        let settings: Self = serde_json::from_slice(&bytes)?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self, root: &Path) -> io::Result<()> {
        self.validate()?;
        std::fs::create_dir_all(root)?;
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        file.write_all(&serde_json::to_vec(self)?)?;
        file.as_file().sync_all()?;
        file.persist(root.join("voice-settings.json")).map_err(|e| e.error)?;
        Ok(())
    }
}

