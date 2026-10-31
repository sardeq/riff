//! Library (boards + sounds) and settings, persisted as JSON.

use crate::hotkeys::Hotkey;
use crate::paths::paths;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SoundFx {
    /// Linear gain, 0..=2
    pub volume: f32,
    /// Playback rate (changes pitch too), 0.25..=3
    pub speed: f32,
    /// Low-shelf gain in dB at ~120 Hz
    pub bass_db: f32,
    /// Echo wet amount 0..=1
    pub echo: f32,
    /// Trim as fractions of the clip length
    pub trim_start: f32,
    pub trim_end: f32,
    /// Seconds
    pub fade_in: f32,
    pub fade_out: f32,
    pub looping: bool,
    /// Send to the virtual microphone
    pub to_mic: bool,
    /// Send to your own speakers/headphones
    pub to_speakers: bool,
}

impl Default for SoundFx {
    fn default() -> Self {
        Self {
            volume: 1.0,
            speed: 1.0,
            bass_db: 0.0,
            echo: 0.0,
            trim_start: 0.0,
            trim_end: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            looping: false,
            to_mic: true,
            to_speakers: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SoundSource {
    #[default]
    Local,
    Builtin,
    Tts {
        text: String,
        voice: String,
    },
    Freesound {
        id: u64,
        username: String,
        license: String,
        url: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Sound {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub board: String,
    pub emoji: String,
    pub color: usize,
    pub fx: SoundFx,
    pub hotkey: Option<Hotkey>,
    pub favorite: bool,
    pub plays: u32,
    pub last_played: i64,
    pub added: i64,
    /// Cached duration in seconds (0 until decoded once)
    pub duration: f32,
    pub source: SoundSource,
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: String::new(),
            path: PathBuf::new(),
            board: String::new(),
            emoji: "🔊".into(),
            color: 0,
            fx: SoundFx::default(),
            hotkey: None,
            favorite: false,
            plays: 0,
            last_played: 0,
            added: now_secs(),
            duration: 0.0,
            source: SoundSource::Local,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Board {
    pub id: String,
    pub name: String,
    pub color: usize,
}

impl Default for Board {
    fn default() -> Self {
        Self { id: new_id(), name: "Board".into(), color: 0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Library {
    pub boards: Vec<Board>,
    pub sounds: Vec<Sound>,
}

impl Library {
    pub fn load() -> Self {
        let mut lib: Library = read_json(&paths().library_file()).unwrap_or_default();
        if lib.boards.is_empty() {
            lib.boards.push(Board { id: new_id(), name: "Main".into(), color: 0 });
        }
        let first = lib.boards[0].id.clone();
        let ids: Vec<String> = lib.boards.iter().map(|b| b.id.clone()).collect();
        for s in &mut lib.sounds {
            if !ids.contains(&s.board) {
                s.board = first.clone();
            }
        }
        lib
    }

    pub fn save(&self) {
        write_json(&paths().library_file(), self);
    }

    pub fn sound(&self, id: &str) -> Option<&Sound> {
        self.sounds.iter().find(|s| s.id == id)
    }

    pub fn sound_mut(&mut self, id: &str) -> Option<&mut Sound> {
        self.sounds.iter_mut().find(|s| s.id == id)
    }

    pub fn board(&self, id: &str) -> Option<&Board> {
        self.boards.iter().find(|b| b.id == id)
    }

    /// Find a sound by id, exact name, or case-insensitive prefix/substring.
    pub fn find(&self, query: &str) -> Option<&Sound> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return None;
        }
        self.sounds
            .iter()
            .find(|s| s.id == query)
            .or_else(|| self.sounds.iter().find(|s| s.name.to_lowercase() == q))
            .or_else(|| self.sounds.iter().find(|s| s.name.to_lowercase().starts_with(&q)))
            .or_else(|| self.sounds.iter().find(|s| s.name.to_lowercase().contains(&q)))
    }

    pub fn find_board(&self, query: &str) -> Option<&Board> {
        let q = query.trim().to_lowercase();
        self.boards
            .iter()
            .find(|b| b.id == query || b.name.to_lowercase() == q)
            .or_else(|| self.boards.iter().find(|b| b.name.to_lowercase().starts_with(&q)))
    }

    /// Delete a sound; also removes its file when Riff owns it.
    pub fn remove_sound(&mut self, id: &str) {
        if let Some(pos) = self.sounds.iter().position(|s| s.id == id) {
            let s = self.sounds.remove(pos);
            if s.path.starts_with(paths().data.as_path()) {
                let _ = std::fs::remove_file(&s.path);
            }
        }
    }

    pub fn remove_board(&mut self, id: &str) {
        if self.boards.len() <= 1 {
            return;
        }
        let ids: Vec<String> = self.sounds.iter().filter(|s| s.board == id).map(|s| s.id.clone()).collect();
        for sid in ids {
            self.remove_sound(&sid);
        }
        self.boards.retain(|b| b.id != id);
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ClickMode {
    /// Clicking a playing pad restarts it
    #[default]
    Restart,
    /// Clicking a playing pad stops it
    Toggle,
    /// Clicking always starts another layered instance
    Layer,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TtsEngine {
    #[default]
    Piper,
    Espeak,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsSettings {
    pub engine: TtsEngine,
    /// Piper voice key, e.g. "en_US-lessac-medium"
    pub piper_voice: String,
    pub piper_speaker: u32,
    /// espeak voice, e.g. "en-us"
    pub espeak_voice: String,
    pub espeak_variant: String,
    /// 0.5..=2.0 (1.0 = normal)
    pub speed: f32,
    /// espeak pitch 0..=99
    pub pitch: u32,
    pub piper_bin: String,
    pub espeak_bin: String,
}

impl Default for TtsSettings {
    fn default() -> Self {
        Self {
            engine: TtsEngine::Piper,
            piper_voice: "en_US-lessac-medium".into(),
            piper_speaker: 0,
            espeak_voice: "en-us".into(),
            espeak_variant: String::new(),
            speed: 1.0,
            pitch: 50,
            piper_bin: "piper".into(),
            espeak_bin: "espeak-ng".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Create the "Riff Microphone" virtual device on launch
    pub virtual_mic: bool,
    /// Volume of sounds going into the virtual mic
    pub mic_bus_volume: f32,
    /// Volume of sounds you hear yourself
    pub monitor_volume: f32,
    /// Mix your real microphone into the virtual mic
    pub mic_passthrough: bool,
    pub mic_gain: f32,
    /// PulseAudio/PipeWire source name; None = system default
    pub input_device: Option<String>,
    /// Sink used for monitoring; None = system default
    pub monitor_device: Option<String>,
    pub duck_mic: bool,
    /// Mic gain multiplier while sounds play
    pub duck_amount: f32,
    pub limiter: bool,
    pub auto_level: bool,
    pub exclusive: bool,
    pub click_mode: ClickMode,
    pub global_hotkeys: bool,
    pub stop_hotkey: Option<Hotkey>,
    pub mic_hotkey: Option<Hotkey>,
    pub copy_imports: bool,
    pub freesound_key: String,
    pub tts: TtsSettings,
    pub pad_size: f32,
    pub accent: usize,
    pub show_inspector: bool,
    pub seeded: bool,
    pub last_board: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            virtual_mic: true,
            mic_bus_volume: 1.0,
            monitor_volume: 0.7,
            mic_passthrough: true,
            mic_gain: 1.0,
            input_device: None,
            monitor_device: None,
            duck_mic: false,
            duck_amount: 0.35,
            limiter: true,
            auto_level: false,
            exclusive: false,
            click_mode: ClickMode::Restart,
            global_hotkeys: true,
            stop_hotkey: None,
            mic_hotkey: None,
            copy_imports: true,
            freesound_key: String::new(),
            tts: TtsSettings::default(),
            pad_size: 1.0,
            accent: 0,
            show_inspector: true,
            seeded: false,
            last_board: String::new(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        read_json(&paths().settings_file()).unwrap_or_default()
    }
    pub fn save(&self) {
        write_json(&paths().settings_file(), self);
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("could not parse {}: {e}", path.display());
            // keep a backup so a bad file never silently eats the user's library
            let _ = std::fs::copy(path, path.with_extension("json.bak"));
            None
        }
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) {
    let tmp = path.with_extension("json.tmp");
    match serde_json::to_string_pretty(value) {
        Ok(text) => {
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
        Err(e) => log::error!("serialize {}: {e}", path.display()),
    }
}

pub const AUDIO_EXTS: &[&str] = &["mp3", "wav", "ogg", "oga", "flac", "m4a", "aac", "mp4", "opus"];

pub fn is_audio_file(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| AUDIO_EXTS.contains(&e.to_lowercase().as_str())).unwrap_or(false)
}

/// Turn "my_cool-sound (final).mp3" into "My cool sound (final)".
pub fn pretty_name(p: &Path) -> String {
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Sound");
    let cleaned: String = stem.replace(['_', '-'], " ");
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut c = cleaned.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => "Sound".into(),
    }
}

/// Pick a fitting emoji from the sound's name.
pub fn guess_emoji(name: &str) -> &'static str {
    let n = name.to_lowercase();
    let table: &[(&[&str], &str)] = &[
        (&["horn", "airhorn", "mlg"], "📯"),
        (&["laugh", "lol", "haha"], "😂"),
        (&["drum", "ba dum", "rimshot"], "🥁"),
        (&["sad", "trombone", "fail"], "😢"),
        (&["bruh"], "😐"),
        (&["wow"], "😮"),
        (&["clap", "applause"], "👏"),
        (&["boom", "explo", "bomb"], "💥"),
        (&["beep", "censor", "bleep"], "🤐"),
        (&["win", "victory", "level", "tada"], "🏆"),
        (&["scream", "ahh"], "😱"),
        (&["cat", "meow"], "🐱"),
        (&["dog", "bark", "woof"], "🐶"),
        (&["music", "song", "beat"], "🎵"),
        (&["guitar", "riff"], "🎸"),
        (&["piano"], "🎹"),
        (&["bell", "ding"], "🔔"),
        (&["fart"], "💨"),
        (&["siren", "alarm"], "🚨"),
        (&["gun", "shot"], "🔫"),
        (&["fire"], "🔥"),
        (&["ghost", "spooky"], "👻"),
        (&["robot", "tts", "say"], "🤖"),
        (&["rain", "storm", "thunder"], "⛈"),
        (&["car", "engine"], "🚗"),
        (&["hello", "hi", "hey"], "👋"),
        (&["no", "nope"], "🙅"),
        (&["yes", "yeah"], "👍"),
    ];
    for (keys, e) in table {
        if keys.iter().any(|k| n.contains(k)) {
            return e;
        }
    }
    const FALLBACK: &[&str] = &["🔊", "🎧", "✨", "⚡", "🎤", "🎶", "💫", "🌀"];
    let h = n.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
    FALLBACK[h % FALLBACK.len()]
}

/// Copy an imported file into Riff's data dir so the library survives moves.
pub fn adopt_file(src: &Path, copy: bool) -> std::io::Result<PathBuf> {
    if !copy {
        return Ok(src.to_path_buf());
    }
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("bin");
    let dest = paths().sounds_dir().join(format!("{}.{}", new_id(), ext.to_lowercase()));
    std::fs::copy(src, &dest)?;
    Ok(dest)
}
