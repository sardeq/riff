//! A handful of procedurally generated starter sounds so a fresh install isn't empty.

use super::OUT_RATE;
use std::f32::consts::TAU;

const SR: f32 = OUT_RATE as f32;

pub struct Starter {
    pub name: &'static str,
    pub emoji: &'static str,
    pub color: usize,
    pub file: &'static str,
    pub render: fn() -> Vec<f32>,
}

pub const STARTERS: &[Starter] = &[
    Starter { name: "Air horn", emoji: "📯", color: 1, file: "airhorn.wav", render: airhorn },
    Starter { name: "Ba dum tss", emoji: "🥁", color: 4, file: "badumtss.wav", render: badumtss },
    Starter { name: "Sad trombone", emoji: "😢", color: 5, file: "sad_trombone.wav", render: sad_trombone },
    Starter { name: "Level up", emoji: "🏆", color: 3, file: "level_up.wav", render: level_up },
    Starter { name: "Censor beep", emoji: "🤐", color: 0, file: "censor.wav", render: censor },
    Starter { name: "Drumroll", emoji: "🎉", color: 6, file: "drumroll.wav", render: drumroll },
    Starter { name: "Laser", emoji: "⚡", color: 2, file: "laser.wav", render: laser },
    Starter { name: "Suspense", emoji: "👻", color: 7, file: "suspense.wav", render: suspense },
];

fn buf(secs: f32) -> Vec<f32> {
    vec![0.0; (secs * SR) as usize * 2]
}

fn add(b: &mut [f32], i: usize, l: f32, r: f32) {
    if i * 2 + 1 < b.len() {
        b[i * 2] += l;
        b[i * 2 + 1] += r;
    }
}

fn saw(phase: f32) -> f32 {
    2.0 * (phase - (phase + 0.5).floor())
}

fn square(phase: f32) -> f32 {
    if phase.fract() < 0.5 { 1.0 } else { -1.0 }
}

struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn normalize(mut b: Vec<f32>, target: f32) -> Vec<f32> {
    let peak = b.iter().fold(0f32, |m, s| m.max(s.abs())).max(1e-6);
    for s in &mut b {
        *s *= target / peak;
    }
    b
}

fn airhorn() -> Vec<f32> {
    let mut b = buf(1.9);
    let blasts = [(0.0, 0.28), (0.36, 0.28), (0.72, 1.05)];
    let freqs = [466.0, 554.0, 698.0, 932.0];
    for (start, len) in blasts {
        let s0 = (start * SR) as usize;
        let n = (len * SR) as usize;
        let mut ph = [0f32; 8];
        for i in 0..n {
            let t = i as f32 / SR;
            let env = (t / 0.015).min(1.0) * ((len - t) / 0.05).clamp(0.0, 1.0);
            let bend = 1.0 - 0.06 * (-t * 30.0).exp(); // scoops up into pitch
            let mut l = 0.0;
            let mut r = 0.0;
            for (k, f) in freqs.iter().enumerate() {
                for (d, det) in [0.997f32, 1.004].iter().enumerate() {
                    let idx = k * 2 + d;
                    ph[idx] = (ph[idx] + f * det * bend / SR).fract();
                    let v = saw(ph[idx]);
                    if d == 0 { l += v } else { r += v }
                }
            }
            add(&mut b, s0 + i, (l * 0.25).tanh() * env, (r * 0.25).tanh() * env);
        }
    }
    normalize(b, 0.85)
}

fn tom(b: &mut [f32], at: f32, f0: f32, noise: &mut Noise) {
    let s0 = (at * SR) as usize;
    let mut ph = 0.0f32;
    for i in 0..(0.35 * SR) as usize {
        let t = i as f32 / SR;
        let f = f0 * (1.0 + 1.5 * (-t * 40.0).exp());
        ph = (ph + f / SR).fract();
        let v = (ph * TAU).sin() * (-t * 9.0).exp() + noise.next() * 0.15 * (-t * 60.0).exp();
        add(b, s0 + i, v, v);
    }
}

fn cymbal(b: &mut [f32], at: f32, len: f32, decay: f32, noise: &mut Noise) {
    let s0 = (at * SR) as usize;
    let mut hp = [0f32; 2];
    let mut prev = [0f32; 2];
    for i in 0..(len * SR) as usize {
        let t = i as f32 / SR;
        let env = (-t * decay).exp();
        let mut out = [0f32; 2];
        for c in 0..2 {
            let x = noise.next();
            hp[c] = 0.92 * (hp[c] + x - prev[c]); // one-pole high-pass for sizzle
            prev[c] = x;
            out[c] = hp[c] * env * 0.6;
        }
        add(b, s0 + i, out[0], out[1]);
    }
}

fn badumtss() -> Vec<f32> {
    let mut b = buf(1.8);
    let mut n = Noise(0x1234_5678);
    tom(&mut b, 0.0, 160.0, &mut n);
    tom(&mut b, 0.18, 110.0, &mut n);
    tom(&mut b, 0.42, 70.0, &mut n);
    cymbal(&mut b, 0.42, 1.35, 3.2, &mut n);
    normalize(b, 0.85)
}

fn sad_trombone() -> Vec<f32> {
    let notes = [(293.66, 0.45), (277.18, 0.45), (261.63, 0.45), (246.94, 1.4)];
    let mut b = buf(3.0);
    let mut at = 0usize;
    let mut ph = 0.0f32;
    for (i, (f, len)) in notes.iter().enumerate() {
        let n = (len * SR) as usize;
        for k in 0..n {
            let t = k as f32 / SR;
            let vib = if i == 3 { 1.0 + 0.025 * (TAU * 5.5 * t).sin() * (t / 0.4).min(1.0) } else { 1.0 };
            ph = (ph + f * vib / SR).fract();
            let env = (t / 0.04).min(1.0) * ((len - t) / 0.12).clamp(0.0, 1.0);
            let v = (saw(ph) * 0.6 + (ph * TAU).sin() * 0.6).tanh() * env;
            add(&mut b, at + k, v, v);
        }
        at += n;
    }
    normalize(b, 0.8)
}

fn level_up() -> Vec<f32> {
    let seq = [523.25, 659.25, 783.99, 1046.5, 783.99, 1046.5, 1318.5];
    let mut b = buf(1.2);
    for (i, f) in seq.iter().enumerate() {
        let s0 = (i as f32 * 0.075 * SR) as usize;
        let len = if i == seq.len() - 1 { 0.5 } else { 0.09 };
        let mut ph = 0.0f32;
        for k in 0..(len * SR) as usize {
            let t = k as f32 / SR;
            ph = (ph + f / SR).fract();
            let env = ((len - t) / len).max(0.0);
            let v = square(ph) * 0.35 * env;
            let pan = (i as f32 / seq.len() as f32) - 0.5;
            add(&mut b, s0 + k, v * (1.0 - pan), v * (1.0 + pan));
        }
    }
    normalize(b, 0.7)
}

fn censor() -> Vec<f32> {
    let mut b = buf(0.7);
    for i in 0..(0.7 * SR) as usize {
        let t = i as f32 / SR;
        let env = (t / 0.005).min(1.0) * ((0.7 - t) / 0.005).clamp(0.0, 1.0);
        let v = (TAU * 1000.0 * t).sin() * 0.6 * env;
        add(&mut b, i, v, v);
    }
    b
}

fn drumroll() -> Vec<f32> {
    let mut b = buf(3.2);
    let mut n = Noise(0xBEEF_CAFE);
    let roll = 2.0;
    let mut t = 0.0f32;
    while t < roll {
        let vel = 0.25 + 0.75 * (t / roll);
        let s0 = (t * SR) as usize;
        for k in 0..(0.06 * SR) as usize {
            let tt = k as f32 / SR;
            let v = n.next() * (-tt * 70.0).exp() * vel + (TAU * 190.0 * tt).sin() * (-tt * 50.0).exp() * vel * 0.6;
            add(&mut b, s0 + k, v * 0.9, v);
        }
        t += 0.045;
    }
    tom(&mut b, roll, 60.0, &mut n);
    cymbal(&mut b, roll, 1.2, 2.5, &mut n);
    normalize(b, 0.85)
}

fn laser() -> Vec<f32> {
    let mut b = buf(0.5);
    let mut ph = 0.0f32;
    for i in 0..(0.45 * SR) as usize {
        let t = i as f32 / SR;
        let f = 2400.0 * (-t * 9.0).exp() + 120.0;
        ph = (ph + f / SR).fract();
        let v = square(ph) * 0.4 * (1.0 - t / 0.45);
        add(&mut b, i, v, v * 0.8);
    }
    normalize(b, 0.7)
}

fn suspense() -> Vec<f32> {
    let mut b = buf(3.5);
    let chord = [110.0, 130.81, 155.56, 233.08];
    let mut ph = [0f32; 4];
    for i in 0..(3.5 * SR) as usize {
        let t = i as f32 / SR;
        let env = (t / 1.2).min(1.0) * ((3.5 - t) / 0.8).clamp(0.0, 1.0);
        let trem = 0.75 + 0.25 * (TAU * 7.0 * t).sin();
        let mut l = 0.0;
        let mut r = 0.0;
        for (k, f) in chord.iter().enumerate() {
            ph[k] = (ph[k] + f * (1.0 + 0.002 * k as f32) / SR).fract();
            let v = saw(ph[k]) * 0.5 + (ph[k] * TAU).sin();
            if k % 2 == 0 {
                l += v
            } else {
                r += v
            }
            if k == 0 {
                r += v * 0.5
            }
        }
        add(&mut b, i, l * env * trem * 0.3, r * env * trem * 0.3);
    }
    normalize(b, 0.75)
}
