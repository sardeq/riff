//! Portable fallback backend (Windows/macOS): plays the monitor mix on the
//! default output. Point a VB-Cable-style device at it for a virtual mic.

use super::mixer::Mixer;
use super::{BLOCK, Cmd, EngineConfig, EngineStatus, OUT_RATE, apply, publish};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Receiver;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

pub fn run(_cfg: EngineConfig, rx: Receiver<Cmd>, status: Arc<Mutex<EngineStatus>>) {
    let mixer = Arc::new(Mutex::new(Mixer::default()));
    let host = cpal::default_host();
    let stream = host.default_output_device().and_then(|dev| {
        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: cpal::SampleRate(OUT_RATE),
            buffer_size: cpal::BufferSize::Default,
        };
        let m = mixer.clone();
        let st = status.clone();
        let mut mic_bus = vec![0f32; BLOCK * 2];
        let mut mon_bus = vec![0f32; BLOCK * 2];
        dev.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let mut mx = m.lock();
                for chunk in data.chunks_mut(BLOCK * 2) {
                    let n = chunk.len();
                    mic_bus.resize(n, 0.0);
                    mon_bus.resize(n, 0.0);
                    mx.render(&mut mic_bus, &mut mon_bus, &[]);
                    chunk.copy_from_slice(&mon_bus);
                }
                publish(&mx, &st);
            },
            |e| log::error!("output stream error: {e}"),
            None,
        )
        .ok()
    });
    {
        let mut st = status.lock();
        st.output_ok = stream.is_some();
        st.virtual_ok = false;
        if stream.is_none() {
            st.error = Some("No audio output device".into());
        }
    }
    if let Some(s) = &stream {
        let _ = s.play();
    }
    while let Ok(cmd) = rx.recv() {
        if let Some(None) = apply(&mut mixer.lock(), cmd) {
            break;
        }
    }
    drop(stream);
    std::thread::sleep(Duration::from_millis(10));
}
