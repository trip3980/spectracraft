//! Shared native analysis primitives. No source samples are retained in packets.
use rustfft::{num_complex::Complex, FftPlanner};
use std::f64::consts::TAU;
pub fn hann(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (TAU * i as f64 / (n - 1).max(1) as f64).cos())
        .collect()
}
pub fn median(x: &[f64]) -> f64 {
    if x.is_empty() {
        return 0.;
    }
    let mut v = x.to_vec();
    v.sort_by(f64::total_cmp);
    (v[v.len() / 2] + v[(v.len() - 1) / 2]) * 0.5
}
pub fn interp(x: &[f64], t: f64) -> f64 {
    let i = (t.max(0.) as usize).min(x.len() - 1);
    let j = (i + 1).min(x.len() - 1);
    x[i] + (x[j] - x[i]) * (t - i as f64).clamp(0., 1.)
}
pub fn centered(x: &[f32], center: usize, n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| {
            (center + i)
                .checked_sub(n / 2)
                .and_then(|j| x.get(j))
                .copied()
                .unwrap_or(0.) as f64
        })
        .collect()
}
pub fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).sqrt()
}
pub fn peaks(x: &[f64]) -> Vec<usize> {
    (1..x.len().saturating_sub(1))
        .filter(|&i| x[i] > x[i - 1] && x[i] >= x[i + 1])
        .collect()
}
pub fn refined_bin(x: &[f64], i: usize) -> f64 {
    let (a, b, c) = (
        x[i - 1].max(1e-20).ln(),
        x[i].max(1e-20).ln(),
        x[i + 1].max(1e-20).ln(),
    );
    i as f64 + (0.5 * (a - c) / (a - 2. * b + c).min(-1e-10)).clamp(-0.5, 0.5)
}
pub struct Random(u64);
impl Random {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn uniform(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn normal(&mut self) -> f64 {
        (-2. * self.uniform().max(1e-16).ln()).sqrt() * (TAU * self.uniform()).cos()
    }
}
pub fn spectrum(x: &[f64], window: &[f64], n: usize) -> Vec<f64> {
    let mut a = vec![Complex::new(0., 0.); n];
    for ((a, x), w) in a.iter_mut().zip(x).zip(window) {
        a.re = x * w;
    }
    FftPlanner::new().plan_fft_forward(n).process(&mut a);
    a[..n / 2 + 1].iter().map(|v| v.norm()).collect()
}

/// Bilinear-transform Butterworth bandpass, direct form II in f64.
/// Zeros at DC/Nyquist, analog prototype poles mapped through the bandpass.
pub fn bandpass(x: &[f64], rate: u32, low: f64, high: f64, order: usize) -> Vec<f64> {
    let fs = rate as f64;
    let lo = 2. * fs * (std::f64::consts::PI * low / fs).tan();
    let hi = 2. * fs * (std::f64::consts::PI * high / fs).tan();
    let width = hi - lo;
    let center = (lo * hi).sqrt();
    let mut poles = Vec::new();
    for k in 0..order {
        let p = Complex::from_polar(
            1.,
            std::f64::consts::PI * (2 * k + 1 + order) as f64 / (2 * order) as f64,
        );
        let b = p * width;
        let discriminant = (b * b - Complex::new(4. * center * center, 0.)).sqrt();
        for s in [(b + discriminant) * 0.5, (b - discriminant) * 0.5] {
            poles.push((Complex::new(2. * fs, 0.) + s) / (Complex::new(2. * fs, 0.) - s));
        }
    }
    let poly = |roots: &[Complex<f64>]| {
        let mut a = vec![Complex::new(1., 0.)];
        for &r in roots {
            let mut b = vec![Complex::new(0., 0.); a.len() + 1];
            for (i, &v) in a.iter().enumerate() {
                b[i] += v;
                b[i + 1] -= r * v;
            }
            a = b;
        }
        a
    };
    let zeros: Vec<_> = (0..order)
        .flat_map(|_| [Complex::new(1., 0.), Complex::new(-1., 0.)])
        .collect();
    let a: Vec<_> = poly(&poles).iter().map(|v| v.re).collect();
    let mut b: Vec<_> = poly(&zeros).iter().map(|v| v.re).collect();
    let at = Complex::from_polar(1., -2. * (center / (2. * fs)).atan());
    let value = |v: &[f64]| {
        v.iter()
            .enumerate()
            .map(|(i, v)| at.powu(i as u32) * v)
            .sum::<Complex<f64>>()
            .norm()
    };
    let gain = value(&a) / value(&b);
    for v in &mut b {
        *v *= gain;
    }
    let mut state = vec![0.; a.len() - 1];
    x.iter()
        .map(|&x| {
            let y = b[0] * x + state[0];
            for i in 0..state.len() - 1 {
                state[i] = state[i + 1] + b[i + 1] * x - a[i + 1] * y;
            }
            let i = state.len() - 1;
            state[i] = b[i + 1] * x - a[i + 1] * y;
            y
        })
        .collect()
}

/// PCG XSL RR 128/64 with the four-word SeedSequence expansion used by the
/// retired experiments. Uniform stream checked against upstream NumPy vectors.
/// Normal draws elsewhere deliberately retain the separately documented RNG.
pub struct ReferenceRandom {
    state: u128,
    increment: u128,
}
impl ReferenceRandom {
    pub fn new(seed: u64) -> Self {
        fn hash(mut x: u32, c: &mut u32) -> u32 {
            x ^= *c;
            *c = c.wrapping_mul(0x931e8875);
            x = x.wrapping_mul(*c);
            x ^ (x >> 16)
        }
        let mut c = 0x43b0d7e5;
        let mut pool = [0u32; 4];
        for (i, v) in pool.iter_mut().enumerate() {
            *v = hash(
                match i {
                    0 => seed as u32,
                    1 => (seed >> 32) as u32,
                    _ => 0,
                },
                &mut c,
            );
        }
        for i in 0..4 {
            for j in 0..4 {
                if i != j {
                    let y = hash(pool[i], &mut c);
                    let x = pool[j]
                        .wrapping_mul(0xca01f9dd)
                        .wrapping_sub(y.wrapping_mul(0x4973f715));
                    pool[j] = x ^ (x >> 16);
                }
            }
        }
        let mut words = [0u32; 8];
        c = 0x8b51f9dd;
        for (i, v) in words.iter_mut().enumerate() {
            let mut x = pool[i % 4] ^ c;
            c = c.wrapping_mul(0x58f38ded);
            x = x.wrapping_mul(c);
            *v = x ^ (x >> 16);
        }
        let word = |i| words[i] as u128 | ((words[i + 1] as u128) << 32);
        let initial = (word(0) << 64) | word(2);
        let seq = (word(4) << 64) | word(6);
        let mut r = Self {
            state: 0,
            increment: (seq << 1) | 1,
        };
        r.step();
        r.state = r.state.wrapping_add(initial);
        r.step();
        r
    }
    fn step(&mut self) {
        self.state = self
            .state
            .wrapping_mul((2549297995355413924u128 << 64) | 4865540595714422341)
            .wrapping_add(self.increment);
    }
    pub fn raw(&mut self) -> u64 {
        self.step();
        ((self.state >> 64) as u64 ^ self.state as u64).rotate_right((self.state >> 122) as u32)
    }
    pub fn uniform(&mut self) -> f64 {
        (self.raw() >> 11) as f64 / (1u64 << 53) as f64
    }
}
#[cfg(test)]
mod reference_rng_tests {
    #[test]
    fn upstream_pcg64_vector() {
        let mut r = super::ReferenceRandom::new(0xdeadbeaf);
        for value in [
            0x60d24054e17a0698,
            0xd5e79d89856e4f12,
            0xd254972fe64bd782,
            0xf1e3072a53c72571,
            0xd7c1d7393d4115c9,
            0x77b75928b763e1e2,
            0xee6dee05190f7909,
        ] {
            assert_eq!(r.raw(), value);
        }
    }
}
