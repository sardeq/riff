//! PulseAudio / PipeWire (pipewire-pulse) backend.
//!
//! Threads:
//! * render (this one): mixes and writes the *mic bus* into the virtual sink,
//!   which paces everything at 48 kHz.
//! * monitor: plays the *monitor bus* on your speakers/headphones.
//! * capture: records your real microphone for passthrough.

use super::mixer::Mixer;
use super::{BLOCK, Cmd, EngineConfig, EngineStatus, OUT_RATE, apply, publish};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use libpulse_binding::def::BufferAttr;
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const SPEC: Spec = Spec { format: Format::F32le, rate: OUT_RATE, channels: 2 };
const FRAME_BYTES: u32 = 8;

fn ms_bytes(ms: u32) -> u32 {
    OUT_RATE / 1000 * ms * FRAME_BYTES
}

fn open_playback(dev: Option<&str>, stream: &str, latency_ms: u32) -> Result<Simple, String> {
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength: ms_bytes(latency_ms),
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: u32::MAX,
    };
    Simple::new(None, "Riff", Direction::Playback, dev, stream, &SPEC, None, Some(&attr))
        .map_err(|e| format!("{stream}: {e}"))
}

fn open_record(dev: Option<&str>) -> Result<Simple, String> {
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength: u32::MAX,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: ms_bytes(10),
    };
    Simple::new(None, "Riff", Direction::Record, dev, "Microphone passthrough", &SPEC, None, Some(&attr))
        .map_err(|e| format!("microphone: {e}"))
}

fn as_bytes(s: &[f32]) -> &[u8] {
    // SAFETY: f32 has no padding/invalid bit patterns and u8 alignment is 1.
    unsafe { std::slice::from_raw_parts(s.as_ptr() as *const u8, std::mem::size_of_val(s)) }
}

struct Worker {
    stop: Arc<AtomicBool>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn spawn_monitor(dev: Option<String>, status: Arc<Mutex<EngineStatus>>) -> (Worker, Sender<Vec<f32>>) {
    let (tx, rx) = bounded::<Vec<f32>>(8);
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    std::thread::Builder::new()
        .name("riff-monitor".into())
        .spawn(move || {
            let out = match open_playback(dev.as_deref(), "Monitor", 40) {
                Ok(o) => o,
                Err(e) => {
                    log::error!("{e}");
                    status.lock().error = Some(format!("Can't open speakers: {e}"));
                    return;
                }
            };
            while !s2.load(Ordering::Relaxed) {
                // When idle we simply stop writing; the server plays silence.
                if let Ok(block) = rx.recv_timeout(Duration::from_millis(100))
                    && out.write(as_bytes(&block)).is_err()
                {
                    break;
                }
            }
        })
        .expect("spawn monitor");
    (Worker { stop }, tx)
}

fn spawn_capture(dev: Option<String>, status: Arc<Mutex<EngineStatus>>) -> (Worker, Receiver<Vec<f32>>) {
    let (tx, rx) = bounded::<Vec<f32>>(16);
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    std::thread::Builder::new()
        .name("riff-capture".into())
        .spawn(move || {
            let rec = match open_record(dev.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("{e}");
                    status.lock().mic_ok = false;
                    return;
                }
            };
            status.lock().mic_ok = true;
            let mut bytes = vec![0u8; BLOCK * FRAME_BYTES as usize];
            while !s2.load(Ordering::Relaxed) {
                if rec.read(&mut bytes).is_err() {
                    status.lock().mic_ok = false;
                    break;
                }
                let block: Vec<f32> =
                    bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
                if let Err(TrySendError::Disconnected(_)) = tx.try_send(block) {
                    break;
                }
            }
        })
        .expect("spawn capture");
    (Worker { stop }, rx)
}

struct Session {
    primary: Option<Simple>,
    routed: bool,
    monitor: Option<(Worker, Sender<Vec<f32>>)>,
    capture: Option<(Worker, Receiver<Vec<f32>>)>,
}

impl Session {
    fn open(cfg: &EngineConfig, status: &Arc<Mutex<EngineStatus>>) -> Self {
        let mut err = None;
        let (primary, routed) = match cfg.virtual_sink.as_deref() {
            Some(sink) => match open_playback(Some(sink), "Soundboard → Riff Microphone", 30) {
                Ok(p) => (Some(p), true),
                Err(e) => {
                    log::warn!("virtual sink unavailable, falling back to speakers: {e}");
                    err = Some(format!("Virtual mic unavailable: {e}"));
                    (open_playback(cfg.monitor_device.as_deref(), "Soundboard", 40).ok(), false)
                }
            },
            None => (open_playback(cfg.monitor_device.as_deref(), "Soundboard", 40).ok(), false),
        };
        let monitor = routed.then(|| spawn_monitor(cfg.monitor_device.clone(), status.clone()));
        let capture = (routed && cfg.mic_passthrough).then(|| spawn_capture(cfg.input_device.clone(), status.clone()));
        let mut st = status.lock();
        st.output_ok = primary.is_some();
        st.virtual_ok = routed && primary.is_some();
        if !(routed && cfg.mic_passthrough) {
            st.mic_ok = false;
        }
        st.error = if primary.is_none() { Some("No audio output available".into()) } else { err };
        drop(st);
        Self { primary, routed, monitor, capture }
    }
}

pub fn run(mut cfg: EngineConfig, rx: Receiver<Cmd>, status: Arc<Mutex<EngineStatus>>) {
    let mut mixer = Mixer::default();
    let mut mic_bus = vec![0f32; BLOCK * 2];
    let mut mon_bus = vec![0f32; BLOCK * 2];
    let mut mic_fifo: VecDeque<f32> = VecDeque::with_capacity(BLOCK * 32);
    let mut mic_block: Vec<f32> = Vec::with_capacity(BLOCK * 2);
    let mut monitor_tail = 0u32;

    'session: loop {
        let session = Session::open(&cfg, &status);
        let opened = Instant::now();
        let mut primed = false;
        mic_fifo.clear();

        loop {
            // 1. commands
            loop {
                match rx.try_recv() {
                    Ok(cmd) => match apply(&mut mixer, cmd) {
                        Some(Some(new_cfg)) => {
                            cfg = new_cfg;
                            continue 'session;
                        }
                        Some(None) => return,
                        None => {}
                    },
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => return,
                }
            }

            // 2. microphone jitter buffer: start after 30 ms, cap at 100 ms (PipeWire delivers in ~20 ms quanta)
            mic_block.clear();
            if let Some((_, cap)) = &session.capture {
                while let Ok(b) = cap.try_recv() {
                    mic_fifo.extend(b);
                }
                while mic_fifo.len() > BLOCK * 2 * 10 {
                    mic_fifo.drain(..BLOCK * 2);
                }
                if !primed && mic_fifo.len() >= BLOCK * 2 * 3 {
                    primed = true;
                }
                if primed {
                    if mic_fifo.len() >= BLOCK * 2 {
                        mic_block.extend(mic_fifo.drain(..BLOCK * 2));
                    } else {
                        primed = false;
                    }
                }
            }
            mixer.mic_enabled = session.capture.is_some();

            // 3. mix
            mixer.render(&mut mic_bus, &mut mon_bus, &mic_block);
            publish(&mixer, &status);

            // 4. output
            match &session.primary {
                Some(out) => {
                    let buf = if session.routed { &mic_bus } else { &mon_bus };
                    if let Err(e) = out.write(as_bytes(buf)) {
                        log::error!("write failed: {e}");
                        status.lock().output_ok = false;
                        std::thread::sleep(Duration::from_millis(500));
                        continue 'session;
                    }
                    if let Some((_, mon)) = &session.monitor {
                        if !mixer.voices.is_empty() {
                            monitor_tail = 30; // keep feeding briefly after the last voice for clean tails
                        }
                        if monitor_tail > 0 {
                            monitor_tail -= 1;
                            let _ = mon.try_send(mon_bus.clone());
                        }
                    }
                }
                None => {
                    std::thread::sleep(Duration::from_millis(10));
                    if opened.elapsed() > Duration::from_secs(3) {
                        continue 'session;
                    }
                }
            }
        }
    }
}
