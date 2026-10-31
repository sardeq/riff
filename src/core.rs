//! Shared application core. Cloned into the UI, the hotkey dispatcher and the
//! IPC server so sounds can be triggered even while the window is hidden.

use crate::audio::clip::{Clip, decode_file};
use crate::audio::mixer::PlayRequest;
use crate::audio::{Cmd, Engine, EngineConfig};
use crate::hotkeys::Hotkey;
use crate::model::{ClickMode, Library, Settings, SoundFx, now_secs};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

pub enum CoreEvent {
    Toast(ToastKind, String),
    HotkeyRecorded(Hotkey),
    ShowWindow,
}

struct LoadJob {
    id: String,
    path: PathBuf,
    play: bool,
}

#[derive(Clone)]
pub struct Core {
    pub lib: Arc<RwLock<Library>>,
    pub settings: Arc<RwLock<Settings>>,
    pub engine: Engine,
    pub clips: Arc<RwLock<HashMap<String, Arc<Clip>>>>,
    pub failed: Arc<RwLock<HashMap<String, String>>>,
    loading: Arc<Mutex<HashSet<String>>>,
    load_tx: Sender<LoadJob>,
    events: Sender<CoreEvent>,
    ctx: Arc<Mutex<Option<egui::Context>>>,
    /// The UI is waiting for a key combo (hotkey recorder)
    pub recording: Arc<AtomicBool>,
    /// Library changed; UI saves it soon
    pub dirty: Arc<AtomicBool>,
}

impl Core {
    pub fn new(lib: Library, settings: Settings, engine_cfg: EngineConfig) -> (Self, Receiver<CoreEvent>) {
        let (load_tx, load_rx) = crossbeam_channel::unbounded::<LoadJob>();
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let core = Self {
            lib: Arc::new(RwLock::new(lib)),
            settings: Arc::new(RwLock::new(settings)),
            engine: Engine::start(engine_cfg),
            clips: Arc::default(),
            failed: Arc::default(),
            loading: Arc::default(),
            load_tx,
            events: ev_tx,
            ctx: Arc::default(),
            recording: Arc::default(),
            dirty: Arc::default(),
        };
        core.push_mix();
        // two decoder threads keep the UI snappy with big libraries
        for n in 0..2 {
            let c = core.clone();
            let rx = load_rx.clone();
            std::thread::Builder::new()
                .name(format!("riff-decode-{n}"))
                .spawn(move || {
                    for job in rx {
                        c.run_load(job);
                    }
                })
                .expect("spawn decoder");
        }
        (core, ev_rx)
    }

    pub fn set_ctx(&self, ctx: egui::Context) {
        *self.ctx.lock() = Some(ctx);
    }

    pub fn repaint(&self) {
        if let Some(c) = self.ctx.lock().as_ref() {
            c.request_repaint();
        }
    }

    pub fn emit(&self, e: CoreEvent) {
        let _ = self.events.send(e);
        self.repaint();
    }

    pub fn toast(&self, kind: ToastKind, msg: impl Into<String>) {
        self.emit(CoreEvent::Toast(kind, msg.into()));
    }

    // ---------- clip cache ----------

    pub fn clip(&self, id: &str) -> Option<Arc<Clip>> {
        self.clips.read().get(id).cloned()
    }

    /// Queue a decode unless it's cached/in flight.
    pub fn ensure_loaded(&self, id: &str) {
        if self.clips.read().contains_key(id) || self.failed.read().contains_key(id) {
            return;
        }
        let path = match self.lib.read().sound(id) {
            Some(s) => s.path.clone(),
            None => return,
        };
        if self.loading.lock().insert(id.to_string()) {
            let _ = self.load_tx.send(LoadJob { id: id.to_string(), path, play: false });
        }
    }

    pub fn invalidate(&self, id: &str) {
        self.clips.write().remove(id);
        self.failed.write().remove(id);
    }

    fn run_load(&self, job: LoadJob) {
        let res = decode_file(&job.path);
        self.loading.lock().remove(&job.id);
        match res {
            Ok(clip) => {
                let dur = clip.duration();
                self.clips.write().insert(job.id.clone(), Arc::new(clip));
                if let Some(s) = self.lib.write().sound_mut(&job.id)
                    && (s.duration - dur).abs() > 0.01
                {
                    s.duration = dur;
                    self.dirty.store(true, Ordering::Relaxed);
                }
                if job.play {
                    self.trigger(&job.id);
                }
            }
            Err(e) => {
                log::warn!("decode {}: {e}", job.path.display());
                self.failed.write().insert(job.id.clone(), e.to_string());
                if job.play {
                    self.toast(ToastKind::Error, format!("Can't play: {e}"));
                }
            }
        }
        self.repaint();
    }

    // ---------- playback ----------

    /// Play a library sound following the user's click mode (restart / toggle / layer).
    pub fn trigger(&self, id: &str) {
        let mode = self.settings.read().click_mode;
        if mode == ClickMode::Toggle && self.engine.is_playing(id) {
            self.engine.send(Cmd::StopSound(id.to_string()));
            return;
        }
        self.play(id, mode != ClickMode::Layer);
    }

    pub fn play(&self, id: &str, restart: bool) {
        let Some((fx, path)) = self.lib.read().sound(id).map(|s| (s.fx.clone(), s.path.clone())) else {
            return;
        };
        let Some(clip) = self.clip(id) else {
            if let Some(err) = self.failed.read().get(id) {
                self.toast(ToastKind::Error, format!("Can't play: {err}"));
                return;
            }
            self.loading.lock().insert(id.to_string());
            let _ = self.load_tx.send(LoadJob { id: id.to_string(), path, play: true });
            return;
        };
        let (auto, exclusive) = {
            let s = self.settings.read();
            (s.auto_level, s.exclusive)
        };
        let gain = if auto { clip.level_gain() } else { 1.0 };
        self.engine.play(PlayRequest { sound_id: id.to_string(), clip, fx, gain, restart, exclusive });
        if let Some(s) = self.lib.write().sound_mut(id) {
            s.plays += 1;
            s.last_played = now_secs();
        }
        self.dirty.store(true, Ordering::Relaxed);
        self.repaint();
    }

    /// Play a clip that isn't in the library (TTS line, Freesound preview).
    pub fn play_adhoc(&self, tag: &str, clip: Arc<Clip>, fx: SoundFx) {
        let exclusive = self.settings.read().exclusive;
        self.engine.play(PlayRequest { sound_id: tag.to_string(), clip, fx, gain: 1.0, restart: true, exclusive });
        self.repaint();
    }

    pub fn stop(&self, id: &str) {
        self.engine.send(Cmd::StopSound(id.to_string()));
    }

    pub fn stop_all(&self) {
        self.engine.stop_all();
        self.repaint();
    }

    pub fn random(&self, board: Option<&str>) -> Option<String> {
        let lib = self.lib.read();
        let pool: Vec<&str> =
            lib.sounds.iter().filter(|s| board.is_none_or(|b| s.board == b)).map(|s| s.id.as_str()).collect();
        if pool.is_empty() {
            return None;
        }
        let id = pool[fastrand::usize(..pool.len())].to_string();
        drop(lib);
        self.play(&id, true);
        Some(id)
    }

    pub fn toggle_mic(&self, on: Option<bool>) -> bool {
        let v = {
            let mut s = self.settings.write();
            s.mic_passthrough = on.unwrap_or(!s.mic_passthrough);
            s.save();
            s.mic_passthrough
        };
        self.reconfigure();
        v
    }

    // ---------- engine config ----------

    pub fn engine_config(&self) -> EngineConfig {
        engine_config(&self.settings.read())
    }

    pub fn reconfigure(&self) {
        self.engine.send(Cmd::Reconfigure(self.engine_config()));
        self.repaint();
    }

    pub fn push_mix(&self) {
        let s = self.settings.read();
        self.engine.send(Cmd::Mix {
            mic_bus: s.mic_bus_volume,
            monitor: s.monitor_volume,
            mic_gain: s.mic_gain,
            duck: s.duck_mic.then_some(s.duck_amount),
            limiter: s.limiter,
        });
    }

    // ---------- hotkeys ----------

    pub fn handle_hotkey(&self, hk: Hotkey) {
        if self.recording.load(Ordering::Relaxed) {
            self.emit(CoreEvent::HotkeyRecorded(hk));
            return;
        }
        let (stop, mic) = {
            let s = self.settings.read();
            (s.stop_hotkey.clone(), s.mic_hotkey.clone())
        };
        if stop.as_ref() == Some(&hk) {
            self.stop_all();
            return;
        }
        if mic.as_ref() == Some(&hk) {
            let on = self.toggle_mic(None);
            self.toast(ToastKind::Info, if on { "Mic passthrough on" } else { "Mic passthrough off" });
            return;
        }
        let ids: Vec<String> =
            self.lib.read().sounds.iter().filter(|s| s.hotkey.as_ref() == Some(&hk)).map(|s| s.id.clone()).collect();
        for id in ids {
            self.trigger(&id);
        }
    }

    // ---------- remote control ----------

    /// Handle one line from `riff <command>` / the control socket.
    pub fn command(&self, line: &str) -> String {
        let line = line.trim();
        let (cmd, arg) = line.split_once(' ').map(|(c, a)| (c, a.trim())).unwrap_or((line, ""));
        match cmd {
            "play" | "toggle" => {
                let id = self.lib.read().find(arg).map(|s| (s.id.clone(), s.name.clone()));
                match id {
                    Some((id, name)) => {
                        if cmd == "toggle" {
                            self.trigger(&id)
                        } else {
                            self.play(&id, true)
                        }
                        format!("ok {name}")
                    }
                    None => format!("err no sound matching '{arg}'"),
                }
            }
            "stop" => {
                if arg.is_empty() {
                    self.stop_all();
                    "ok".into()
                } else {
                    match self.lib.read().find(arg) {
                        Some(s) => {
                            self.stop(&s.id);
                            "ok".into()
                        }
                        None => format!("err no sound matching '{arg}'"),
                    }
                }
            }
            "stop-all" => {
                self.stop_all();
                "ok".into()
            }
            "pause" => {
                self.engine.send(Cmd::Pause(None, None));
                "ok".into()
            }
            "random" => {
                let board = if arg.is_empty() { None } else { self.lib.read().find_board(arg).map(|b| b.id.clone()) };
                match self.random(board.as_deref()) {
                    Some(id) => {
                        format!("ok {}", self.lib.read().sound(&id).map(|s| s.name.clone()).unwrap_or_default())
                    }
                    None => "err no sounds".into(),
                }
            }
            "mic" => {
                let on = match arg {
                    "on" => Some(true),
                    "off" => Some(false),
                    _ => None,
                };
                let v = self.toggle_mic(on);
                format!("ok mic {}", if v { "on" } else { "off" })
            }
            "say" => {
                if arg.is_empty() {
                    return "err nothing to say".into();
                }
                let tts = self.settings.read().tts.clone();
                match crate::tts::synthesize(&tts, arg) {
                    Ok(sp) => {
                        self.play_adhoc("tts", Arc::new(sp.clip), SoundFx::default());
                        "ok".into()
                    }
                    Err(e) => format!("err {e}"),
                }
            }
            "list" => {
                let lib = self.lib.read();
                let mut out = String::from("ok\n");
                for b in &lib.boards {
                    out.push_str(&format!("# {}\n", b.name));
                    for s in lib.sounds.iter().filter(|s| s.board == b.id) {
                        let hk = s.hotkey.as_ref().map(|h| format!("  [{}]", h.label())).unwrap_or_default();
                        out.push_str(&format!("  {}{hk}\n", s.name));
                    }
                }
                out
            }
            "show" => {
                self.emit(CoreEvent::ShowWindow);
                "ok".into()
            }
            "ping" => "ok pong".into(),
            _ => format!("err unknown command '{cmd}' (try: play, toggle, stop, random, mic, say, list, show)"),
        }
    }
}

#[cfg(target_os = "linux")]
pub fn engine_config(s: &Settings) -> EngineConfig {
    use crate::audio::routing;
    let mut input = s.input_device.clone();
    if input.is_none() && routing::default_source().as_deref() == Some(routing::SOURCE) {
        // Default input is Riff itself — never record our own output (feedback loop).
        input = routing::input_devices().first().map(|d| d.name.clone());
    }
    let mut monitor = s.monitor_device.clone();
    if monitor.is_none() && routing::default_sink().as_deref() == Some(routing::SINK) {
        monitor = routing::output_devices().first().map(|d| d.name.clone());
    }
    EngineConfig {
        virtual_sink: s.virtual_mic.then(|| routing::SINK.to_string()),
        monitor_device: monitor,
        input_device: input,
        mic_passthrough: s.mic_passthrough,
    }
}

#[cfg(not(target_os = "linux"))]
pub fn engine_config(s: &Settings) -> EngineConfig {
    EngineConfig {
        virtual_sink: None,
        monitor_device: s.monitor_device.clone(),
        input_device: s.input_device.clone(),
        mic_passthrough: false,
    }
}
