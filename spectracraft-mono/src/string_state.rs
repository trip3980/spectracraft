//! A damped string whose vibration survives bends and re-picks.
use std::f64::consts::TAU;

#[derive(Clone)]
pub(crate) struct Mode {
    harmonic: f64,
    loss: f64,
    coupling: f64,
    re: f64,
    im: f64,
}

impl Mode {
    pub(crate) fn new(harmonic: f64, coupling: f64, decay: f64, rate: f64) -> Self {
        Self {
            harmonic,
            loss: (-decay / rate).exp(),
            coupling,
            re: 0.,
            im: 0.,
        }
    }
    pub(crate) fn advance(&mut self, f: f64, rate: f64, kick: f64) -> f64 {
        let hz = self.harmonic * f;
        if hz >= rate * 0.5 {
            self.re = 0.;
            self.im = 0.;
            return 0.;
        }
        let (s, c) = (TAU * hz / rate).sin_cos();
        let (re, im) = (self.re, self.im);
        self.re = self.loss * (c * re - s * im) + self.coupling * kick;
        self.im = self.loss * (s * re + c * im);
        self.im
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bend_rotates_old_energy_and_repick_adds_without_reset() {
        let mut note = Mode::new(1., 0.8, 1.2, 48000.);
        note.advance(220., 48000., 1.);
        for i in 0..1200 {
            note.advance(220. + i as f64 / 40., 48000., 0.);
        }
        let energy = note.re.hypot(note.im);
        assert!((energy - 0.8 * (-1.2_f64 * 1200. / 48000.).exp()).abs() < 1e-10);
        let mut baseline = note.clone();
        let a = baseline.advance(250., 48000., 0.);
        let b = note.advance(250., 48000., 0.5);
        assert_eq!(a, b);
        assert!((note.re - baseline.re - 0.4).abs() < 1e-12);
        assert!((note.advance(250., 48000., 0.) - baseline.advance(250., 48000., 0.)).abs() > 0.01);
    }
}
