//! Decoded audio clips (any format symphonia understands) + waveform peaks.

use anyhow::{Context, Result, anyhow};
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymErr;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const PEAKS: usize = 160;

/// Interleaved stereo f32 at the file's native sample rate.
pub struct Clip {
    pub data: Vec<f32>,
    pub rate: u32,
    pub frames: usize,
    /// Normalised (0..1) peak envelope for drawing waveforms
    pub peaks: Vec<f32>,
    /// RMS of the whole clip, used for auto-levelling
    pub rms: f32,
}

impl Clip {
    pub fn duration(&self) -> f32 {
        self.frames as f32 / self.rate.max(1) as f32
    }

    pub fn from_stereo(data: Vec<f32>, rate: u32) -> Self {
        let frames = data.len() / 2;
        let mut peaks = vec![0f32; PEAKS];
        let mut sum_sq = 0f64;
        {
            for (i, fr) in data.chunks_exact(2).enumerate() {
                let v = fr[0].abs().max(fr[1].abs());
                let b = (i * PEAKS).checked_div(frames).unwrap_or(0);
                if v > peaks[b] {
                    peaks[b] = v;
                }
                sum_sq += (fr[0] as f64 * fr[0] as f64 + fr[1] as f64 * fr[1] as f64) * 0.5;
            }
        }
        let max = peaks.iter().cloned().fold(0.0f32, f32::max).max(1e-6);
        for p in &mut peaks {
            *p = (*p / max).sqrt(); // sqrt makes quiet detail visible
        }
        let rms = if frames > 0 { (sum_sq / frames as f64).sqrt() as f32 } else { 0.0 };
        Self { data, rate, frames, peaks, rms }
    }

    /// Gain that brings this clip to a common loudness (clamped so silence isn't boosted to noise).
    pub fn level_gain(&self) -> f32 {
        const TARGET: f32 = 0.12;
        if self.rms <= 1e-5 { 1.0 } else { (TARGET / self.rms).clamp(0.25, 4.0) }
    }
}

pub fn decode_file(path: &Path) -> Result<Clip> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let ext = path.extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase());
    match decode_source(Box::new(file), ext.as_deref()) {
        Ok(c) => Ok(c),
        // Opus, WMA, video soundtracks… let ffmpeg handle anything symphonia can't, when installed.
        Err(e) => decode_with_ffmpeg(path).map_err(|_| e),
    }
}

fn decode_with_ffmpeg(path: &Path) -> Result<Clip> {
    let out = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "2", "-ar", "48000", "-f", "wav", "-"])
        .output()?;
    if !out.status.success() || out.stdout.len() < 64 {
        return Err(anyhow!("ffmpeg could not decode the file"));
    }
    decode_bytes(out.stdout, Some("wav"))
}

pub fn decode_bytes(bytes: Vec<u8>, ext: Option<&str>) -> Result<Clip> {
    decode_source(Box::new(std::io::Cursor::new(bytes)), ext)
}

fn decode_source(src: Box<dyn MediaSource>, ext: Option<&str>) -> Result<Clip> {
    let mss = MediaSourceStream::new(src, Default::default());
    let mut hint = Hint::new();
    if let Some(e) = ext {
        hint.with_extension(e);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| anyhow!("unsupported or corrupt audio: {e}"))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("no audio track"))?;
    let track_id = track.id;
    let mut rate = track.codec_params.sample_rate.unwrap_or(48_000);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("no decoder: {e}"))?;

    let mut out: Vec<f32> = Vec::new();
    let mut buf: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymErr::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymErr::ResetRequired) => break,
            Err(e) => {
                if out.is_empty() {
                    return Err(anyhow!("read error: {e}"));
                }
                break;
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                rate = spec.rate;
                let ch = spec.channels.count().max(1);
                let need = decoded.capacity() as u64;
                if buf.as_ref().map(|b| b.capacity() < need as usize * ch).unwrap_or(true) {
                    buf = Some(SampleBuffer::<f32>::new(need, spec));
                }
                let b = buf.as_mut().unwrap();
                b.copy_interleaved_ref(decoded);
                let s = b.samples();
                if ch == 1 {
                    out.reserve(s.len() * 2);
                    for &v in s {
                        out.push(v);
                        out.push(v);
                    }
                } else {
                    out.reserve(s.len() / ch * 2);
                    for fr in s.chunks_exact(ch) {
                        out.push(fr[0]);
                        out.push(fr[1]);
                    }
                }
            }
            Err(SymErr::DecodeError(_)) => continue,
            Err(SymErr::IoError(_)) => break,
            Err(e) => return Err(anyhow!("decode error: {e}")),
        }
    }
    if out.is_empty() {
        return Err(anyhow!("file contains no audio"));
    }
    Ok(Clip::from_stereo(out, rate))
}

/// Encode interleaved stereo f32 to a 16-bit WAV file.
pub fn write_wav(path: &Path, data: &[f32], rate: u32) -> std::io::Result<()> {
    let mut bytes = Vec::with_capacity(44 + data.len() * 2);
    let data_len = (data.len() * 2) as u32;
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&2u16.to_le_bytes()); // stereo
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for &s in data {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_round_trip() {
        let data: Vec<f32> = (0..9600).map(|i| ((i / 2) as f32 * 0.01).sin() * 0.5).collect();
        let path = std::env::temp_dir().join(format!("riff-test-{}.wav", std::process::id()));
        write_wav(&path, &data, 48_000).unwrap();
        let clip = decode_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(clip.rate, 48_000);
        assert_eq!(clip.frames, 4800);
        assert!((clip.duration() - 0.1).abs() < 1e-3);
        assert_eq!(clip.peaks.len(), PEAKS);
    }

    /// Decodes whatever compressed files exist in $RIFF_TEST_AUDIO (mp3/ogg/flac/m4a…).
    #[test]
    fn decodes_common_formats() {
        let Some(dir) = std::env::var_os("RIFF_TEST_AUDIO") else { return };
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let clip = decode_file(&p).unwrap_or_else(|err| panic!("{}: {err}", p.display()));
            assert!(clip.duration() > 0.5, "{}: {}", p.display(), clip.duration());
        }
    }
}
