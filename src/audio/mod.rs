//! Audio engine front-end. The UI, hotkeys and IPC all talk to the engine
//! through a cheap-to-clone [`Engine`] handle.

pub mod clip;
pub mod mixer;
pub mod synth;

#[cfg(not(target_os = "linux"))]
mod cpal_backend;
#[cfg(target_os = "linux")]
mod pulse;
#[cfg(target_os = "linux")]
pub mod routing;

use crossbeam_channel::{Receiver, Sender};
use mixer::{Mixer, PlayRequest};
use parking_lot::Mutex;
use std::sync::Arc;

pub const OUT_RATE: u32 = 48_000;
/// 10 ms blocks
pub const BLOCK: usize = 480;

#[derive(Clone, Debug, Default)]
pub struct VoiceStatus {
    pub voice_id: u64,
    pub sound_id: String,
    pub progress: f32,
    pub position: f32,
    pub duration: f32,
    pub paused: bool,
    pub looping: bool,
}

#[derive(Clone, Debug, Default)]
pub struct EngineStatus {
    pub voices: Vec<VoiceStatus>,
    pub peak_mic_bus: f32,
    pub peak_monitor: f32,
    pub peak_mic_in: f32,
    /// Sounds are reaching the virtual microphone
    pub virtual_ok: bool,
    pub output_ok: bool,
    pub mic_ok: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EngineConfig {
    /// Sink that feeds the virtual mic (None = no virtual mic, play to default output)
    pub virtual_sink: Option<String>,
    /// Where you hear sounds (None = default sink)
    pub monitor_device: Option<String>,
    /// Real mic to mix in (None = default source)
    pub input_device: Option<String>,
    pub mic_passthrough: bool,
}

pub enum Cmd {
    Play(PlayRequest),
    StopSound(String),
    StopVoice(u64),
    StopAll,
    /// None = all sounds; None bool = toggle
    Pause(Option<String>, Option<bool>),
    Mix {
        mic_bus: f32,
        monitor: f32,
        mic_gain: f32,
        duck: Option<f32>,
        limiter: bool,
    },
    Reconfigure(EngineConfig),
    Shutdown,
}

#[derive(Clone)]
pub struct Engine {
    tx: Sender<Cmd>,
    status: Arc<Mutex<EngineStatus>>,
}

impl Engine {
    pub fn start(cfg: EngineConfig) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let status = Arc::new(Mutex::new(EngineStatus::default()));
        let st = status.clone();
        std::thread::Builder::new()
            .name("riff-audio".into())
            .spawn(move || run_backend(cfg, rx, st))
            .expect("spawn audio thread");
        Self { tx, status }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn play(&self, req: PlayRequest) {
        self.send(Cmd::Play(req));
    }

    pub fn stop_all(&self) {
        self.send(Cmd::StopAll);
    }

    pub fn status(&self) -> EngineStatus {
        self.status.lock().clone()
    }

    pub fn is_playing(&self, sound_id: &str) -> bool {
        self.status.lock().voices.iter().any(|v| v.sound_id == sound_id)
    }
}

/// Apply a command to the mixer. Returns a new config if the backend must reconnect.
fn apply(mixer: &mut Mixer, cmd: Cmd) -> Option<Option<EngineConfig>> {
    match cmd {
        Cmd::Play(r) => {
            mixer.play(r);
        }
        Cmd::StopSound(s) => mixer.stop_sound(&s),
        Cmd::StopVoice(v) => mixer.stop_voice(v),
        Cmd::StopAll => mixer.stop_all(),
        Cmd::Pause(s, p) => mixer.set_paused(s.as_deref(), p),
        Cmd::Mix { mic_bus, monitor, mic_gain, duck, limiter } => {
            mixer.mic_bus_volume = mic_bus;
            mixer.monitor_volume = monitor;
            mixer.mic_gain = mic_gain;
            mixer.duck = duck;
            mixer.limiter = limiter;
        }
        Cmd::Reconfigure(c) => return Some(Some(c)),
        Cmd::Shutdown => return Some(None),
    }
    None
}

fn publish(mixer: &Mixer, status: &Mutex<EngineStatus>) {
    let mut st = status.lock();
    st.voices = mixer.status();
    st.peak_mic_bus = mixer.peak_mic_bus;
    st.peak_monitor = mixer.peak_monitor;
    st.peak_mic_in = mixer.peak_mic_in;
}

#[cfg(target_os = "linux")]
fn run_backend(cfg: EngineConfig, rx: Receiver<Cmd>, status: Arc<Mutex<EngineStatus>>) {
    pulse::run(cfg, rx, status);
}

#[cfg(not(target_os = "linux"))]
fn run_backend(cfg: EngineConfig, rx: Receiver<Cmd>, status: Arc<Mutex<EngineStatus>>) {
    cpal_backend::run(cfg, rx, status);
}
