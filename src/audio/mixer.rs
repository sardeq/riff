//! The real-time mixer: voices with per-sound effects, two output buses
//! (virtual mic + your own monitor), microphone passthrough, ducking and a limiter.

use super::clip::Clip;
use super::{OUT_RATE, VoiceStatus};
use crate::model::SoundFx;
use std::sync::Arc;

#[derive(Clone)]
pub struct PlayRequest {
    pub sound_id: String,
    pub clip: Arc<Clip>,
    pub fx: SoundFx,
    /// Extra gain (auto-level), multiplied with fx.volume
    pub gain: f32,
    /// Stop other voices of the same sound first
    pub restart: bool,
    /// Stop every other voice first
    pub exclusive: bool,
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: [f32; 2],
    z2: [f32; 2],
}

impl Biquad {
    /// RBJ cookbook low shelf.
    fn low_shelf(freq: f32, db: f32, rate: f32) -> Self {
        let a = 10f32.powf(db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq / rate;
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / 2.0 * std::f32::consts::SQRT_2;
        let sa = 2.0 * a.sqrt() * alpha;
        let b0 = a * ((a + 1.0) - (a - 1.0) * cs + sa);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cs);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cs - sa);
        let a0 = (a + 1.0) + (a - 1.0) * cs + sa;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cs);
        let a2 = (a + 1.0) + (a - 1.0) * cs - sa;
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, ..Default::default() }
    }

    #[inline]
    fn process(&mut self, ch: usize, x: f32) -> f32 {
        // transposed direct form II
        let y = self.b0 * x + self.z1[ch];
        self.z1[ch] = self.b1 * x - self.a1 * y + self.z2[ch];
        self.z2[ch] = self.b2 * x - self.a2 * y;
        y
    }
}

struct Echo {
    buf: Vec<f32>,
    pos: usize,
    wet: f32,
}

impl Echo {
    fn new(wet: f32) -> Self {
        let frames = (OUT_RATE as f32 * 0.24) as usize;
        Self { buf: vec![0.0; frames * 2], pos: 0, wet }
    }
    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let dl = self.buf[self.pos];
        let dr = self.buf[self.pos + 1];
        // ping-pong feedback for a wider sound
        self.buf[self.pos] = l + dr * 0.45;
        self.buf[self.pos + 1] = r + dl * 0.45;
        self.pos = (self.pos + 2) % self.buf.len();
        (l + dl * self.wet, r + dr * self.wet)
    }
}

pub struct Voice {
    pub id: u64,
    pub sound_id: String,
    clip: Arc<Clip>,
    pos: f64,
    step: f64,
    start: f64,
    end: f64,
    gain: f32,
    looping: bool,
    fade_in: f64,
    fade_out: f64,
    to_mic: bool,
    to_monitor: bool,
    bass: Option<Biquad>,
    echo: Option<Echo>,
    paused: bool,
    /// Short de-click ramp when stopping (frames left)
    stopping: Option<u32>,
    done: bool,
}

const STOP_RAMP: u32 = 480; // 10 ms

impl Voice {
    fn new(id: u64, req: PlayRequest) -> Self {
        let frames = req.clip.frames as f64;
        let fx = &req.fx;
        let ts = fx.trim_start.clamp(0.0, 1.0) as f64;
        let te = fx.trim_end.clamp(0.0, 1.0) as f64;
        let (ts, te) = if te - ts < 0.001 { (0.0, 1.0) } else { (ts, te) };
        let start = ts * frames;
        let end = (te * frames).max(start + 1.0);
        let step = req.clip.rate as f64 / OUT_RATE as f64 * fx.speed.clamp(0.1, 4.0) as f64;
        Self {
            id,
            sound_id: req.sound_id,
            pos: start,
            step,
            start,
            end,
            gain: fx.volume.max(0.0) * req.gain,
            looping: fx.looping,
            fade_in: fx.fade_in.max(0.0) as f64 * req.clip.rate as f64,
            fade_out: fx.fade_out.max(0.0) as f64 * req.clip.rate as f64,
            to_mic: fx.to_mic,
            to_monitor: fx.to_speakers,
            bass: (fx.bass_db.abs() > 0.1).then(|| Biquad::low_shelf(120.0, fx.bass_db, OUT_RATE as f32)),
            echo: (fx.echo > 0.01).then(|| Echo::new(fx.echo.min(1.0))),
            paused: false,
            stopping: None,
            clip: req.clip,
            done: false,
        }
    }

    #[inline]
    fn frame(&self, i: isize) -> (f32, f32) {
        let n = self.clip.frames as isize;
        let i = i.clamp(0, n - 1) as usize;
        (self.clip.data[i * 2], self.clip.data[i * 2 + 1])
    }

    /// Cubic (Catmull-Rom) interpolated stereo sample at fractional position.
    #[inline]
    fn sample(&self, pos: f64) -> (f32, f32) {
        let i = pos.floor() as isize;
        let t = (pos - i as f64) as f32;
        let (l0, r0) = self.frame(i - 1);
        let (l1, r1) = self.frame(i);
        let (l2, r2) = self.frame(i + 1);
        let (l3, r3) = self.frame(i + 2);
        let cr = |p0: f32, p1: f32, p2: f32, p3: f32| {
            p1 + 0.5 * t * (p2 - p0 + t * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + t * (3.0 * (p1 - p2) + p3 - p0)))
        };
        (cr(l0, l1, l2, l3), cr(r0, r1, r2, r3))
    }

    fn envelope(&self) -> f32 {
        let mut g = 1.0f64;
        if self.fade_in > 0.0 && !self.looping {
            g = g.min((self.pos - self.start) / self.fade_in);
        }
        if self.fade_out > 0.0 && !self.looping {
            g = g.min((self.end - self.pos) / self.fade_out);
        }
        g.clamp(0.0, 1.0) as f32
    }

    pub fn progress(&self) -> f32 {
        ((self.pos - self.start) / (self.end - self.start).max(1.0)).clamp(0.0, 1.0) as f32
    }

    pub fn stop(&mut self) {
        if self.stopping.is_none() {
            self.stopping = Some(STOP_RAMP);
        }
    }
}

pub struct Mixer {
    pub voices: Vec<Voice>,
    next_id: u64,
    pub mic_bus_volume: f32,
    pub monitor_volume: f32,
    pub mic_gain: f32,
    pub mic_enabled: bool,
    pub duck: Option<f32>,
    pub limiter: bool,
    duck_env: f32,
    lim_env: [f32; 2],
    pub peak_mic_bus: f32,
    pub peak_monitor: f32,
    pub peak_mic_in: f32,
}

impl Default for Mixer {
    fn default() -> Self {
        Self {
            voices: Vec::new(),
            next_id: 1,
            mic_bus_volume: 1.0,
            monitor_volume: 0.7,
            mic_gain: 1.0,
            mic_enabled: true,
            duck: None,
            limiter: true,
            duck_env: 1.0,
            lim_env: [1.0; 2],
            peak_mic_bus: 0.0,
            peak_monitor: 0.0,
            peak_mic_in: 0.0,
        }
    }
}

impl Mixer {
    pub fn play(&mut self, req: PlayRequest) -> u64 {
        if req.exclusive {
            self.voices.iter_mut().for_each(Voice::stop);
        } else if req.restart {
            self.voices.iter_mut().filter(|v| v.sound_id == req.sound_id).for_each(Voice::stop);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.voices.push(Voice::new(id, req));
        id
    }

    pub fn stop_sound(&mut self, sound_id: &str) {
        self.voices.iter_mut().filter(|v| v.sound_id == sound_id).for_each(Voice::stop);
    }

    pub fn stop_voice(&mut self, id: u64) {
        self.voices.iter_mut().filter(|v| v.id == id).for_each(Voice::stop);
    }

    pub fn stop_all(&mut self) {
        self.voices.iter_mut().for_each(Voice::stop);
    }

    pub fn set_paused(&mut self, sound_id: Option<&str>, paused: Option<bool>) {
        for v in &mut self.voices {
            if sound_id.is_none_or(|s| s == v.sound_id) {
                v.paused = paused.unwrap_or(!v.paused);
            }
        }
    }

    /// Render `frames` stereo frames into both buses. `mic` holds microphone
    /// input frames (may be shorter or empty).
    pub fn render(&mut self, mic_bus: &mut [f32], mon_bus: &mut [f32], mic: &[f32]) {
        mic_bus.fill(0.0);
        mon_bus.fill(0.0);
        let frames = mic_bus.len() / 2;

        let any_to_mic = self.voices.iter().any(|v| v.to_mic && !v.paused && v.stopping.is_none());

        for v in &mut self.voices {
            if v.paused || v.done {
                continue;
            }
            for f in 0..frames {
                if v.pos >= v.end {
                    if v.looping && v.stopping.is_none() {
                        v.pos = v.start + (v.pos - v.end);
                    } else {
                        v.done = true;
                        break;
                    }
                }
                let (mut l, mut r) = v.sample(v.pos);
                let mut g = v.gain * v.envelope();
                if let Some(n) = v.stopping.as_mut() {
                    if *n == 0 {
                        v.done = true;
                        break;
                    }
                    g *= *n as f32 / STOP_RAMP as f32;
                    *n -= 1;
                }
                if let Some(bq) = v.bass.as_mut() {
                    l = bq.process(0, l);
                    r = bq.process(1, r);
                }
                if let Some(e) = v.echo.as_mut() {
                    (l, r) = e.process(l, r);
                }
                l *= g;
                r *= g;
                if v.to_mic {
                    mic_bus[f * 2] += l;
                    mic_bus[f * 2 + 1] += r;
                }
                if v.to_monitor {
                    mon_bus[f * 2] += l;
                    mon_bus[f * 2 + 1] += r;
                }
                v.pos += v.step;
            }
        }
        self.voices.retain(|v| !v.done);

        // bus volumes
        for s in mic_bus.iter_mut() {
            *s *= self.mic_bus_volume;
        }
        for s in mon_bus.iter_mut() {
            *s *= self.monitor_volume;
        }

        // microphone passthrough (+ ducking while sounds play)
        let mut mic_peak = 0f32;
        if self.mic_enabled && !mic.is_empty() {
            let target = match self.duck {
                Some(d) if any_to_mic => d,
                _ => 1.0,
            };
            let n = (mic.len() / 2).min(frames);
            for f in 0..n {
                // ~30 ms attack/release
                self.duck_env += (target - self.duck_env) * 0.0007;
                let g = self.mic_gain * self.duck_env;
                let (l, r) = (mic[f * 2] * g, mic[f * 2 + 1] * g);
                mic_peak = mic_peak.max(l.abs()).max(r.abs());
                mic_bus[f * 2] += l;
                mic_bus[f * 2 + 1] += r;
            }
        }

        if self.limiter {
            limit(mic_bus, &mut self.lim_env[0]);
            limit(mon_bus, &mut self.lim_env[1]);
        }

        let peak = |b: &[f32]| b.iter().fold(0f32, |m, s| m.max(s.abs()));
        self.peak_mic_bus = decay(self.peak_mic_bus, peak(mic_bus));
        self.peak_monitor = decay(self.peak_monitor, peak(mon_bus));
        self.peak_mic_in = decay(self.peak_mic_in, mic_peak);
    }

    pub fn status(&self) -> Vec<VoiceStatus> {
        self.voices
            .iter()
            .filter(|v| v.stopping.is_none())
            .map(|v| {
                // real-time length = source frames / (output rate * step)
                let duration = ((v.end - v.start) / (v.step * OUT_RATE as f64)) as f32;
                let progress = v.progress();
                VoiceStatus {
                    voice_id: v.id,
                    sound_id: v.sound_id.clone(),
                    progress,
                    position: progress * duration,
                    duration,
                    paused: v.paused,
                    looping: v.looping,
                }
            })
            .collect()
    }
}

fn decay(prev: f32, now: f32) -> f32 {
    if now > prev { now } else { prev * 0.85 + now * 0.15 }
}

/// Simple look-ahead-free peak limiter with soft knee: keeps output under 0 dBFS
/// without the harsh crackle of hard clipping.
fn limit(buf: &mut [f32], env: &mut f32) {
    const CEIL: f32 = 0.97;
    for fr in buf.chunks_exact_mut(2) {
        let p = fr[0].abs().max(fr[1].abs());
        let want = if p * *env > CEIL { CEIL / p } else { 1.0 };
        if want < *env {
            *env = want; // instant attack
        } else {
            *env += (1.0 - *env) * 0.0004; // ~50 ms release
        }
        fr[0] = soft(fr[0] * *env);
        fr[1] = soft(fr[1] * *env);
    }
}

#[inline]
fn soft(x: f32) -> f32 {
    if x.abs() < 0.9 { x } else { x.signum() * (0.9 + 0.1 * ((x.abs() - 0.9) / 0.1).tanh()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, rate: u32) -> Arc<Clip> {
        let n = (secs * rate as f32) as usize;
        let mut d = Vec::with_capacity(n * 2);
        for i in 0..n {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5;
            d.push(v);
            d.push(v);
        }
        Arc::new(Clip::from_stereo(d, rate))
    }

    fn req(clip: Arc<Clip>, fx: SoundFx) -> PlayRequest {
        PlayRequest { sound_id: "t".into(), clip, fx, gain: 1.0, restart: true, exclusive: false }
    }

    #[test]
    fn plays_to_both_buses_and_finishes() {
        let mut m = Mixer { monitor_volume: 1.0, ..Default::default() };
        m.play(req(tone(0.1, 44_100), SoundFx::default()));
        let (mut a, mut b) = (vec![0.0; 960], vec![0.0; 960]);
        m.render(&mut a, &mut b, &[]);
        assert!(a.iter().any(|s| s.abs() > 0.1) && b.iter().any(|s| s.abs() > 0.1));
        for _ in 0..20 {
            m.render(&mut a, &mut b, &[]);
        }
        assert!(m.voices.is_empty(), "voice should end after its clip");
    }

    #[test]
    fn routing_flags_are_respected() {
        let mut m = Mixer::default();
        m.play(req(tone(0.2, 48_000), SoundFx { to_speakers: false, ..Default::default() }));
        let (mut a, mut b) = (vec![0.0; 960], vec![0.0; 960]);
        m.render(&mut a, &mut b, &[]);
        assert!(a.iter().any(|s| s.abs() > 0.1));
        assert!(b.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn speed_shortens_duration_and_loop_keeps_going() {
        let mut m = Mixer::default();
        m.play(req(tone(1.0, 48_000), SoundFx { speed: 2.0, ..Default::default() }));
        let d = m.status()[0].duration;
        assert!((d - 0.5).abs() < 0.01, "duration {d}");
        let mut m = Mixer::default();
        m.play(req(tone(0.05, 48_000), SoundFx { looping: true, ..Default::default() }));
        let (mut a, mut b) = (vec![0.0; 960], vec![0.0; 960]);
        for _ in 0..50 {
            m.render(&mut a, &mut b, &[]);
        }
        assert_eq!(m.voices.len(), 1);
        m.stop_all();
        for _ in 0..3 {
            m.render(&mut a, &mut b, &[]);
        }
        assert!(m.voices.is_empty());
    }

    #[test]
    fn limiter_keeps_output_below_full_scale() {
        let mut m = Mixer::default();
        for _ in 0..8 {
            let mut r = req(tone(0.2, 48_000), SoundFx { volume: 2.0, ..Default::default() });
            r.restart = false;
            m.play(r);
        }
        let (mut a, mut b) = (vec![0.0; 960], vec![0.0; 960]);
        m.render(&mut a, &mut b, &[]);
        assert!(a.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn mic_passthrough_and_ducking() {
        let mut m = Mixer { duck: Some(0.2), ..Default::default() };
        let mic = vec![0.5f32; 960];
        let (mut a, mut b) = (vec![0.0; 960], vec![0.0; 960]);
        m.render(&mut a, &mut b, &mic);
        assert!((a[958] - 0.5).abs() < 0.01, "mic should pass through: {}", a[958]);
        assert!(b.iter().all(|s| *s == 0.0), "mic must not reach the monitor");
        m.play(req(tone(1.0, 48_000), SoundFx { volume: 0.0, ..Default::default() }));
        for _ in 0..30 {
            m.render(&mut a, &mut b, &mic);
        }
        assert!(a[958].abs() < 0.15, "mic should be ducked: {}", a[958]);
    }
}
