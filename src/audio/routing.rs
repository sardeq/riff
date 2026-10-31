//! Virtual microphone on PipeWire / PulseAudio, created with `pactl`.
//!
//! Layout:
//!   Riff ──► [riff_sink  "Riff Soundboard" (null sink)]
//!                      └─ .monitor ──► [riff_mic "Riff Microphone" (remapped source)]
//!
//! Voice apps (Discord, games, OBS) pick **Riff Microphone** as their input.
//! A remapped source is used instead of the raw monitor because many apps hide
//! monitor sources from their device lists.

use anyhow::{Result, anyhow};
use std::process::Command;

pub const SINK: &str = "riff_sink";
pub const SOURCE: &str = "riff_mic";

#[derive(Clone, Debug)]
pub struct Device {
    pub name: String,
    pub description: String,
}

fn pactl(args: &[&str]) -> Result<String> {
    let out = Command::new("pactl")
        .args(args)
        .output()
        .map_err(|e| anyhow!("pactl not found ({e}). Install pulseaudio utils / pipewire-pulse."))?;
    if !out.status.success() {
        return Err(anyhow!("pactl {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Owns the loaded modules and unloads them on drop.
pub struct VirtualMic {
    modules: Vec<u32>,
}

impl VirtualMic {
    pub fn create() -> Result<Self> {
        remove_stale();
        let mut modules = Vec::new();
        let sink = load(&[
            "module-null-sink",
            &format!("sink_name={SINK}"),
            "rate=48000",
            "channels=2",
            "sink_properties='device.description=\"Riff Soundboard\"'",
        ])
        .or_else(|_| load(&["module-null-sink", &format!("sink_name={SINK}"), "rate=48000", "channels=2"]))?;
        modules.push(sink);
        let src = load(&[
            "module-remap-source",
            &format!("master={SINK}.monitor"),
            &format!("source_name={SOURCE}"),
            "source_properties='device.description=\"Riff Microphone\"'",
        ])
        .or_else(|_| {
            load(&["module-remap-source", &format!("master={SINK}.monitor"), &format!("source_name={SOURCE}")])
        });
        match src {
            Ok(id) => modules.push(id),
            Err(e) => log::warn!("remap-source failed, apps can still use 'Monitor of Riff Soundboard': {e}"),
        }
        log::info!("virtual microphone ready (modules {modules:?})");
        Ok(Self { modules })
    }
}

impl Drop for VirtualMic {
    fn drop(&mut self) {
        for id in self.modules.iter().rev() {
            let _ = pactl(&["unload-module", &id.to_string()]);
        }
    }
}

fn load(args: &[&str]) -> Result<u32> {
    let mut a = vec!["load-module"];
    a.extend_from_slice(args);
    let out = pactl(&a)?;
    out.trim().parse::<u32>().map_err(|_| anyhow!("unexpected pactl output: {out}"))
}

/// Unload modules left behind by a crashed Riff.
fn remove_stale() {
    if let Ok(list) = pactl(&["list", "short", "modules"]) {
        for line in list.lines() {
            if (line.contains(&format!("sink_name={SINK}")) || line.contains(&format!("source_name={SOURCE}")))
                && let Some(id) = line.split_whitespace().next()
            {
                let _ = pactl(&["unload-module", id]);
            }
        }
    }
}

fn list(kind: &str) -> Vec<Device> {
    // Prefer JSON (pactl ≥ 16) for nice descriptions, fall back to the short format.
    if let Ok(json) = pactl(&["--format=json", "list", kind])
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&json)
        && let Some(arr) = v.as_array()
    {
        return arr
            .iter()
            .filter_map(|d| {
                let name = d.get("name")?.as_str()?.to_string();
                let description = d.get("description").and_then(|x| x.as_str()).unwrap_or(&name).to_string();
                Some(Device { name, description })
            })
            .collect();
    }
    pactl(&["list", "short", kind])
        .map(|s| {
            s.lines()
                .filter_map(|l| l.split('\t').nth(1))
                .map(|n| Device { name: n.to_string(), description: n.to_string() })
                .collect()
        })
        .unwrap_or_default()
}

/// Real microphones (no monitors, not Riff's own devices).
pub fn input_devices() -> Vec<Device> {
    list("sources").into_iter().filter(|d| !d.name.ends_with(".monitor") && d.name != SOURCE).collect()
}

/// Real outputs (not Riff's own sink).
pub fn output_devices() -> Vec<Device> {
    list("sinks").into_iter().filter(|d| d.name != SINK).collect()
}

pub fn default_source() -> Option<String> {
    pactl(&["get-default-source"]).ok().map(|s| s.trim().to_string())
}

pub fn default_sink() -> Option<String> {
    pactl(&["get-default-sink"]).ok().map(|s| s.trim().to_string())
}
