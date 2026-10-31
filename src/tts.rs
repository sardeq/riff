//! Local text-to-speech: Piper (neural voices) and espeak-ng (instant, tiny).

use crate::audio::clip::{Clip, decode_bytes, decode_file};
use crate::model::{TtsEngine, TtsSettings, new_id};
use crate::net;
use crate::paths::paths;
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicU32;

const HF_BASE: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/main/";

/// Voices shown at the top of the picker: good quality, small-ish downloads.
pub const FEATURED: &[&str] = &[
    "en_US-lessac-medium",
    "en_US-ryan-high",
    "en_US-amy-medium",
    "en_GB-alan-medium",
    "en_US-libritts_r-medium",
    "ar_JO-kareem-medium",
    "de_DE-thorsten-medium",
    "fr_FR-siwis-medium",
    "es_ES-davefx-medium",
];

#[derive(Clone, Debug, Deserialize)]
pub struct PiperLanguage {
    pub code: String,
    #[serde(default)]
    pub name_english: String,
    #[serde(default)]
    pub country_english: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PiperFile {
    #[serde(default)]
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PiperVoice {
    pub key: String,
    pub name: String,
    pub language: PiperLanguage,
    pub quality: String,
    #[serde(default)]
    pub num_speakers: u32,
    #[serde(default)]
    pub files: BTreeMap<String, PiperFile>,
}

impl PiperVoice {
    pub fn model_url(&self) -> Option<(String, String)> {
        let onnx = self.files.keys().find(|k| k.ends_with(".onnx"))?;
        let json = self.files.keys().find(|k| k.ends_with(".onnx.json"))?;
        Some((format!("{HF_BASE}{onnx}"), format!("{HF_BASE}{json}")))
    }
    pub fn size_mb(&self) -> f32 {
        self.files.iter().filter(|(k, _)| k.ends_with(".onnx")).map(|(_, f)| f.size_bytes).sum::<u64>() as f32
            / 1_048_576.0
    }
    pub fn label(&self) -> String {
        let mut n = self.name.replace('_', " ");
        if let Some(c) = n.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        format!("{n} · {} ({}) · {}", self.language.name_english, self.language.country_english, self.quality)
    }
}

pub fn model_path(key: &str) -> PathBuf {
    paths().voices_dir().join(format!("{key}.onnx"))
}

pub fn is_installed(key: &str) -> bool {
    model_path(key).exists() && model_path(key).with_extension("onnx.json").exists()
}

pub fn installed_voices() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(paths().voices_dir())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let p = e.path();
                    (p.extension()? == "onnx").then(|| p.file_stem()?.to_str().map(String::from))?
                })
                .filter(|k| is_installed(k))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Fetch (and cache) the official Piper voice catalog.
pub fn fetch_catalog() -> Result<Vec<PiperVoice>> {
    let cache = paths().cache.join("piper-voices.json");
    let fresh = std::fs::metadata(&cache)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().map(|e| e.as_secs() < 7 * 86400).unwrap_or(false))
        .unwrap_or(false);
    let bytes = if fresh {
        std::fs::read(&cache)?
    } else {
        let b = net::get_bytes(&format!("{HF_BASE}voices.json"))?;
        let _ = std::fs::write(&cache, &b);
        b
    };
    let map: BTreeMap<String, PiperVoice> = serde_json::from_slice(&bytes).context("parse voices.json")?;
    let mut voices: Vec<PiperVoice> = map.into_values().collect();
    voices.sort_by(|a, b| {
        let fa = FEATURED.iter().position(|k| *k == a.key).unwrap_or(usize::MAX);
        let fb = FEATURED.iter().position(|k| *k == b.key).unwrap_or(usize::MAX);
        fa.cmp(&fb).then(a.language.code.cmp(&b.language.code)).then(a.key.cmp(&b.key))
    });
    Ok(voices)
}

pub fn download_voice(v: &PiperVoice, progress: &AtomicU32) -> Result<()> {
    let (onnx, json) = v.model_url().ok_or_else(|| anyhow!("voice has no model files"))?;
    let dest = model_path(&v.key);
    net::download(&json, &dest.with_extension("onnx.json"), None)?;
    net::download(&onnx, &dest, Some(progress))?;
    Ok(())
}

pub fn which(bin: &str) -> bool {
    if bin.contains('/') {
        return std::path::Path::new(bin).exists();
    }
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file())).unwrap_or(false)
}

#[derive(Clone, Debug)]
pub struct EspeakVoice {
    pub id: String,
    pub name: String,
}

pub fn espeak_voices(bin: &str) -> Vec<EspeakVoice> {
    let Ok(out) = Command::new(bin).arg("--voices").output() else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut v: Vec<EspeakVoice> = text
        .lines()
        .skip(1)
        .filter_map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            (parts.len() >= 4).then(|| EspeakVoice { id: parts[1].to_string(), name: parts[3].replace('_', " ") })
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v.dedup_by(|a, b| a.id == b.id);
    v
}

pub const ESPEAK_VARIANTS: &[(&str, &str)] = &[
    ("", "Default"),
    ("m3", "Male 3"),
    ("m7", "Male 7"),
    ("f2", "Female 2"),
    ("f4", "Female 4"),
    ("croak", "Croak"),
    ("whisper", "Whisper"),
    ("klatt", "Retro (Klatt)"),
];

pub struct Speech {
    pub clip: Clip,
    /// WAV bytes, so the line can be saved as a pad
    pub wav: Vec<u8>,
    pub voice_label: String,
}

pub fn synthesize(s: &TtsSettings, text: &str) -> Result<Speech> {
    let text = text.trim();
    if text.is_empty() {
        return Err(anyhow!("nothing to say"));
    }
    match s.engine {
        TtsEngine::Espeak => espeak(s, text),
        TtsEngine::Piper => {
            // No Piper or no voice yet? Still say it — with espeak — rather than failing.
            let piper_ready = which(&s.piper_bin) && is_installed(&s.piper_voice);
            if !piper_ready && which(&s.espeak_bin) {
                return espeak(s, text);
            }
            piper(s, text)
        }
    }
}

fn espeak(s: &TtsSettings, text: &str) -> Result<Speech> {
    if !which(&s.espeak_bin) {
        return Err(anyhow!("espeak-ng not found — add `espeak-ng` to your packages"));
    }
    let voice = if s.espeak_variant.is_empty() {
        s.espeak_voice.clone()
    } else {
        format!("{}+{}", s.espeak_voice, s.espeak_variant)
    };
    let wpm = (175.0 * s.speed.clamp(0.3, 3.0)) as u32;
    let mut child = Command::new(&s.espeak_bin)
        .args(["--stdout", "-v", &voice, "-s", &wpm.to_string(), "-p", &s.pitch.min(99).to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("run espeak-ng")?;
    child.stdin.take().unwrap().write_all(text.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() || out.stdout.len() < 64 {
        return Err(anyhow!("espeak-ng failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let wav = out.stdout;
    let clip = decode_bytes(wav.clone(), Some("wav"))?;
    Ok(Speech { clip, wav, voice_label: format!("espeak · {voice}") })
}

fn piper(s: &TtsSettings, text: &str) -> Result<Speech> {
    if !which(&s.piper_bin) {
        return Err(anyhow!("Piper not found — add `piper-tts` to your packages, or switch to espeak"));
    }
    if !is_installed(&s.piper_voice) {
        return Err(anyhow!("Voice '{}' isn't downloaded yet", s.piper_voice));
    }
    let tmp = std::env::temp_dir().join(format!("riff-tts-{}.wav", new_id()));
    let length = format!("{:.3}", 1.0 / s.speed.clamp(0.3, 3.0));
    let model = model_path(&s.piper_voice);
    let run = |length_flag: &str| -> Result<std::process::Output> {
        let mut cmd = Command::new(&s.piper_bin);
        cmd.arg("-m").arg(&model).arg("-f").arg(&tmp).arg(length_flag).arg(&length);
        if s.piper_speaker > 0 {
            cmd.arg("--speaker").arg(s.piper_speaker.to_string());
        }
        let mut child =
            cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().context("run piper")?;
        child.stdin.take().unwrap().write_all(text.as_bytes())?;
        Ok(child.wait_with_output()?)
    };
    // piper1 (Python) uses --length-scale, the original C++ piper uses --length_scale
    let mut out = run("--length-scale")?;
    if !out.status.success() {
        out = run("--length_scale")?;
    }
    if !out.status.success() || !tmp.exists() {
        return Err(anyhow!("piper failed: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("")));
    }
    let wav = std::fs::read(&tmp)?;
    let _ = std::fs::remove_file(&tmp);
    let clip = decode_bytes(wav.clone(), Some("wav"))?;
    Ok(Speech { clip, wav, voice_label: format!("piper · {}", s.piper_voice) })
}

/// Persist a spoken line as a WAV in Riff's data dir.
pub fn save_wav(wav: &[u8]) -> Result<PathBuf> {
    let path = paths().tts_dir().join(format!("{}.wav", new_id()));
    std::fs::write(&path, wav)?;
    // make sure it decodes before we hand it to the library
    decode_file(&path)?;
    Ok(path)
}
