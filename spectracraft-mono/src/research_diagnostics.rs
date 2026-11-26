//! Mathematical provenance records and diagnostic-only FFT energy verification.
//!
//! This module implements:
//! 1. Structured provenance types separating established mathematical identities
//!    from proof-derived constructions and RH consequences.
//! 2. An explicit record of the Alpöge–Claude (Nov 2026) Riemann Hypothesis
//!    proof investigation, honestly recording `ResearchStatus::NoApplicableMethod`
//!    for SpectraCraft audio analysis rather than fabricating placebo controls.
//! 3. Diagnostic-only Parseval / Rayleigh FFT energy consistency verification
//!    matching `rustfft`'s unnormalized forward DFT (`forward_scale = 1.0`).
//!
//! Invariant: none of the diagnostics in this module alter audio samples,
//! normalize signals, mutate analysis cues, or add a sixth processing stage.

use crate::Result;
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};

/// Exact diagnostic label used for Parseval/Rayleigh checks.
/// Strictly attributed to established harmonic analysis, never to RH.
pub const FFT_ENERGY_CONSISTENCY_LABEL: &str = "FFT energy consistency";

/// Unnormalized forward DFT scale used by `rustfft::FftPlanner::plan_fft_forward`:
/// `X[k] = sum_{n=0}^{N-1} x[n] * exp(-i * 2*pi*k*n / N)` (`a = 1.0`).
pub const RUSTFFT_FORWARD_SCALE: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MathematicalOrigin {
    EstablishedIdentity,
    ConstructionFromProof,
    ConsequenceOfRh,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MathematicalSource {
    pub id: String,
    pub label: String,
    pub title: String,
    pub origin: MathematicalOrigin,
    pub reference_url: String,
    pub version_or_commit: String,
    pub theorem_or_section: String,
    pub license_status: String,
    pub notes: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResearchStatus {
    NoApplicableMethod,
    AlgorithmSpecified,
    NumericalValidationPassed,
    AudioEvaluationPassed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RhResearchRecord {
    pub status: ResearchStatus,
    pub primary_sources: Vec<MathematicalSource>,
    pub mathematical_summary: String,
    pub missing_audio_link: String,
    pub production_audio_modified: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnergyConsistencyReport {
    pub label: String,
    pub origin: MathematicalOrigin,
    pub transform_len: usize,
    pub forward_scale: f64,
    pub one_sided: bool,
    pub time_energy: f64,
    pub spectral_energy: f64,
    pub absolute_error: f64,
    pub relative_error: Option<f64>,
    pub allowed_error: f64,
    pub passes: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EvidenceProposal<T> {
    Candidate { value: T, limitations: Vec<String> },
    Abstained { reason: String },
}

pub trait ExperimentalEvidenceMethod {
    type Features: ?Sized;
    type Proposals;
    type Error;

    fn source(&self) -> &MathematicalSource;

    fn propose(
        &self,
        features: &Self::Features,
    ) -> std::result::Result<EvidenceProposal<Self::Proposals>, Self::Error>;
}

/// Established reference for the discrete Parseval / Rayleigh energy theorem.
pub fn parseval_energy_source() -> MathematicalSource {
    MathematicalSource {
        id: "parseval_rayleigh_dft_energy".to_string(),
        label: FFT_ENERGY_CONSISTENCY_LABEL.to_string(),
        title: "Rayleigh Energy Theorem (Parseval's Theorem for the DFT) — Julius O. Smith III, Mathematics of the DFT".to_string(),
        origin: MathematicalOrigin::EstablishedIdentity,
        reference_url: "https://dsprelated.com/freebooks/mdft/Rayleigh_Energy_Theorem_Parseval_s.html".to_string(),
        version_or_commit: "Established classical identity (independent of RH)".to_string(),
        theorem_or_section: "Rayleigh Energy Theorem / Parseval's Theorem (special case of the DFT Power Theorem)".to_string(),
        license_status: "Classical mathematical theorem; independently implemented in Rust".to_string(),
        notes: "For forward DFT X[k] = a * sum_{n=0}^{N-1} x[n] exp(-i 2*pi*k*n/N), sum_n |x[n]|^2 == (1 / (N * a^2)) * sum_k |X[k]|^2. Matches rustfft 6.4.1 with a = 1.0.".to_string(),
    }
}

/// Documented investigation record of the Nov 2026 Alpöge–Claude Lean-verified
/// Riemann Hypothesis / GRH proof and why no finite audio-analysis algorithm is
/// derived from it for SpectraCraft.
pub fn alpoge_claude_rh_investigation_record() -> RhResearchRecord {
    RhResearchRecord {
        status: ResearchStatus::NoApplicableMethod,
        primary_sources: vec![
            MathematicalSource {
                id: "alpoge_2026_remark".to_string(),
                label: "Alpöge–Claude RH/GRH announcement".to_string(),
                title: "A Remark on the Riemann Hypothesis (Levent Alpöge)".to_string(),
                origin: MathematicalOrigin::ConstructionFromProof,
                reference_url: "https://alpo.ge/remark.pdf".to_string(),
                version_or_commit: "Published 2026-11-20".to_string(),
                theorem_or_section: "Main announcement: RH and GRH for Dirichlet L-functions in Lean 4 (~175,187 files, ~17.3M lines)".to_string(),
                license_status: "Original paper / submission published without an explicit open-source license; not copied".to_string(),
                notes: "Original author announcement of the formal Lean 4 proof.".to_string(),
            },
            MathematicalSource {
                id: "leanprover_2026_joint_statement".to_string(),
                label: "Lean FRO & Mathlib verification statement".to_string(),
                title: "Formal Verification Statement on the Riemann Hypothesis Proof".to_string(),
                origin: MathematicalOrigin::ConstructionFromProof,
                reference_url: "https://leanprover-community.github.io/blog/posts/riemann-hypothesis/".to_string(),
                version_or_commit: "Published 2026-11-21 (Lean/Mathlib v4.33.1)".to_string(),
                theorem_or_section: "Mathlib.NumberTheory.LSeries.RiemannHypothesis; axioms propext, Classical.choice, Quot.sound".to_string(),
                license_status: "Verification report only; no code copied".to_string(),
                notes: "Confirms zero sorrys and agreement with Mathlib's canonical statement via comparator.".to_string(),
            },
            MathematicalSource {
                id: "mit_sutherland_2026_grh".to_string(),
                label: "MIT independent verification records".to_string(),
                title: "Independent Verification of the Alpöge–Claude RH/GRH Formalization (Andrew V. Sutherland)".to_string(),
                origin: MathematicalOrigin::ConstructionFromProof,
                reference_url: "https://math.mit.edu/~drew/grh/".to_string(),
                version_or_commit: "Nov 2026 verification runs (nanoda, con-leche, con-ron, lean4lean)".to_string(),
                theorem_or_section: "Independent kernel & external-checker verification logs".to_string(),
                license_status: "Public verification record; no code copied".to_string(),
                notes: "Independent confirmation across multiple Lean checkers.".to_string(),
            },
            MathematicalSource {
                id: "leanprover_community_rh_repo".to_string(),
                label: "Community Lean formalization mirror".to_string(),
                title: "leanprover-community/riemann-hypothesis".to_string(),
                origin: MathematicalOrigin::ConstructionFromProof,
                reference_url: "https://github.com/leanprover-community/riemann-hypothesis".to_string(),
                version_or_commit: "Mirrored submission commit 40e56bab (Lean v4.33.1)".to_string(),
                theorem_or_section: "Community verification harness and mirrored formal proof".to_string(),
                license_status: "Original submission has NO explicit open-source license (redistribution restricted); selected formal-conjectures helpers are Apache-2.0. Kept strictly outside SpectraCraft dependencies.".to_string(),
                notes: "Distinguished from original submission; Lean formalization is never linked into SpectraCraft.".to_string(),
            },
            MathematicalSource {
                id: "waterloo_rubinstein_2026_exposition".to_string(),
                label: "Working mathematical exposition".to_string(),
                title: "Working Mathematical Exposition of the Alpöge–Claude GRH Proof (Michael Rubinstein)".to_string(),
                origin: MathematicalOrigin::ConstructionFromProof,
                reference_url: "https://www.math.uwaterloo.ca/~mrubinst/alpoge_claude_grh/".to_string(),
                version_or_commit: "Nov 2026 working exposition".to_string(),
                theorem_or_section: "Central inequality (M), one-sided smoothed prime-power sum on [Y, Y^{3/2}], theta-correspondence multiplicity amplification, Arakelov period bounds".to_string(),
                license_status: "Expository reference only; no code copied".to_string(),
                notes: "Working human-readable exposition, distinct from the formal Lean artifact.".to_string(),
            },
        ],
        mathematical_summary: "The Alpöge–Claude proof establishes RH and GRH by proving an asymptotic one-sided upper bound for a smoothed sum over prime powers in [Y, Y^{3/2}] via a central inequality (M) with cutoff m ~ x, replacing classical Stepanov multiplicity with repeated theta correspondences between unitary groups to amplify divisibility of automorphic periods at inert primes and bridging back to analytic bounds via Arakelov geometry.".to_string(),
        missing_audio_link: "None of the novel constructions in the proof (unitary-group theta lifts, inert-prime divisibility amplification of automorphic periods, Arakelov height/period bounds, or asymptotic prime-power zero-free inequalities as Y -> infinity) define a finite discrete-time operator, spectral estimator, pitch/partial tracker, or harmonic-disambiguation rule on bounded sampled PCM waveforms x[n]. Meanwhile, Fourier duality, smooth windowing, and Parseval/Rayleigh energy conservation are established classical harmonic analysis independent of RH.".to_string(),
        production_audio_modified: false,
    }
}

/// Honest experimental-method stub reflecting `ResearchStatus::NoApplicableMethod`.
/// Always abstains rather than injecting a placebo RH-labeled heuristic into audio cues.
#[derive(Clone, Debug)]
pub struct NoApplicableRhEvidenceMethod {
    source: MathematicalSource,
}

impl Default for NoApplicableRhEvidenceMethod {
    fn default() -> Self {
        Self {
            source: MathematicalSource {
                id: "alpoge_claude_rh_abstention".to_string(),
                label: "Alpöge–Claude RH proof investigation".to_string(),
                title: "No Applicable Audio-Analysis Method Identified".to_string(),
                origin: MathematicalOrigin::ConstructionFromProof,
                reference_url: "https://alpo.ge/remark.pdf".to_string(),
                version_or_commit: "2026-11-20 (commit 40e56bab verification mirror)".to_string(),
                theorem_or_section: "Abstention under Algorithm Contract (NoApplicableMethod)".to_string(),
                license_status: "No external code copied; independent abstention record".to_string(),
                notes: "Abstains cleanly until a peer-checked finite algorithm with a genuine mathematical mapping to audio analysis exists.".to_string(),
            },
        }
    }
}

impl ExperimentalEvidenceMethod for NoApplicableRhEvidenceMethod {
    type Features = [f32];
    type Proposals = ();
    type Error = std::convert::Infallible;

    fn source(&self) -> &MathematicalSource {
        &self.source
    }

    fn propose(
        &self,
        _features: &Self::Features,
    ) -> std::result::Result<EvidenceProposal<Self::Proposals>, Self::Error> {
        Ok(EvidenceProposal::Abstained {
            reason: "ResearchStatus::NoApplicableMethod — no finite audio-analysis construction from the Alpöge–Claude RH proof has been established.".to_string(),
        })
    }
}

/// Tolerance configuration for Parseval / Rayleigh FFT energy verification.
///
/// Comparison rule:
/// `allowed_error = absolute_tolerance + relative_tolerance * max(time_energy, spectral_energy)`
/// `passes = |time_energy - spectral_energy| <= allowed_error`
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FftToleranceConfig {
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
}

impl FftToleranceConfig {
    /// Justified tolerance for `rustfft::FftPlanner::<f64>` transforms of length `n`.
    ///
    /// Rationale: IEEE-754 `f64` machine epsilon is ~2.22e-16. Radix-2 and
    /// mixed-radix/Bluestein FFT algorithms accumulate `O(log2(N))` rounding
    /// error in amplitude and ~2x that in squared energy. Using a factor of
    /// `128 * f64::EPSILON * max(1, log2(N))` stays tight (~3.1e-13 at N=2048)
    /// while safely tolerating worst-case mixed-radix / Bluestein odd lengths
    /// and catching any factor-of-2, sqrt(N), or N normalization mistake.
    pub fn for_f64_fft(n: usize) -> Self {
        let log_n = (n.max(2) as f64).log2().max(1.0);
        Self {
            absolute_tolerance: 1e-15,
            relative_tolerance: 128.0 * f64::EPSILON * log_n,
        }
    }

    /// Justified tolerance for `rustfft::FftPlanner::<f32>` transforms of length `n`.
    ///
    /// Rationale: IEEE-754 `f32` machine epsilon is ~1.192e-7. Even though
    /// Parseval accumulation itself runs in compensated `f64`, the FFT butterfly
    /// outputs are quantized to `f32` with `O(log2(N))` rounding growth. Using
    /// `64 * f32::EPSILON * max(1, log2(N))` (~8.4e-5 at N=2048) avoids false
    /// positives on long/odd `f32` frames while reliably rejecting scaling or
    /// one-sided Nyquist weighting errors.
    pub fn for_f32_fft(n: usize) -> Self {
        let log_n = (n.max(2) as f64).log2().max(1.0);
        Self {
            absolute_tolerance: 1e-11,
            relative_tolerance: 64.0 * (f32::EPSILON as f64) * log_n,
        }
    }

    fn validate(self) -> Result<()> {
        if !self.absolute_tolerance.is_finite()
            || self.absolute_tolerance < 0.0
            || !self.relative_tolerance.is_finite()
            || self.relative_tolerance < 0.0
        {
            return Err("FFT energy tolerances must be finite and non-negative".into());
        }
        Ok(())
    }
}

/// Neumaier-compensated `f64` accumulator for accurate energy summation.
#[derive(Clone, Copy, Debug, Default)]
struct NeumaierAcc {
    sum: f64,
    comp: f64,
}

impl NeumaierAcc {
    #[inline]
    fn add(&mut self, value: f64) {
        let t = self.sum + value;
        if self.sum.abs() >= value.abs() {
            self.comp += (self.sum - t) + value;
        } else {
            self.comp += (value - t) + self.sum;
        }
        self.sum = t;
    }

    #[inline]
    fn finish(self) -> f64 {
        self.sum + self.comp
    }
}

/// Neumaier-compensated `f64` summation for accurate energy accumulation across
/// long buffers and wide dynamic ranges.
pub fn neumaier_sum<I>(values: I) -> f64
where
    I: IntoIterator<Item = f64>,
{
    let mut acc = NeumaierAcc::default();
    for value in values {
        acc.add(value);
    }
    acc.finish()
}

fn real_energy_f64(samples: &[f64], err_msg: &'static str) -> Result<f64> {
    let mut acc = NeumaierAcc::default();
    for &v in samples {
        if !v.is_finite() {
            return Err(err_msg.into());
        }
        acc.add(v * v);
    }
    Ok(acc.finish())
}

fn complex_energy_f64(bins: &[Complex<f64>], err_msg: &'static str) -> Result<f64> {
    let mut acc = NeumaierAcc::default();
    for z in bins {
        if !z.re.is_finite() || !z.im.is_finite() {
            return Err(err_msg.into());
        }
        acc.add(z.re * z.re + z.im * z.im);
    }
    Ok(acc.finish())
}

fn one_sided_energy_f64(bins: &[Complex<f64>], n: usize, err_msg: &'static str) -> Result<f64> {
    let is_even = n.is_multiple_of(2);
    let nyquist_bin = n / 2;
    let mut acc = NeumaierAcc::default();
    for (k, z) in bins.iter().enumerate() {
        if !z.re.is_finite() || !z.im.is_finite() {
            return Err(err_msg.into());
        }
        let weight = if k == 0 || (is_even && k == nyquist_bin) {
            1.0
        } else {
            2.0
        };
        acc.add(weight * (z.re * z.re + z.im * z.im));
    }
    Ok(acc.finish())
}

fn validate_denom(n: usize, forward_scale: f64) -> Result<f64> {
    if !forward_scale.is_finite() || forward_scale <= 0.0 {
        return Err("FFT forward_scale must be finite and strictly positive".into());
    }
    let denom = (n as f64) * forward_scale * forward_scale;
    if !denom.is_finite() || denom <= 0.0 {
        return Err("FFT normalization denominator is not finite and positive".into());
    }
    Ok(denom)
}

fn build_report(
    transform_len: usize,
    forward_scale: f64,
    one_sided: bool,
    time_energy: f64,
    spectral_energy: f64,
    tol: FftToleranceConfig,
) -> Result<EnergyConsistencyReport> {
    if !time_energy.is_finite() || !spectral_energy.is_finite() {
        return Err("Computed FFT energy overflowed to a non-finite value".into());
    }
    let scale = time_energy.max(spectral_energy);
    let absolute_error = (time_energy - spectral_energy).abs();
    let relative_error = if scale > 0.0 {
        Some(absolute_error / scale)
    } else {
        None
    };
    let allowed_error = tol.absolute_tolerance + tol.relative_tolerance * scale;
    let passes = absolute_error <= allowed_error;

    Ok(EnergyConsistencyReport {
        label: FFT_ENERGY_CONSISTENCY_LABEL.to_string(),
        origin: MathematicalOrigin::EstablishedIdentity,
        transform_len,
        forward_scale,
        one_sided,
        time_energy,
        spectral_energy,
        absolute_error,
        relative_error,
        allowed_error,
        passes,
    })
}

/// Verify Parseval / Rayleigh energy consistency for a complete complex input
/// `x[0..N]` and its complete complex spectrum `X[0..N]`:
/// `sum_{n=0}^{N-1} |x[n]|^2 == (1 / (N * a^2)) * sum_{k=0}^{N-1} |X[k]|^2`.
pub fn verify_complex_spectrum_energy(
    time_frame: &[Complex<f64>],
    spectrum: &[Complex<f64>],
    forward_scale: f64,
    tol: FftToleranceConfig,
) -> Result<EnergyConsistencyReport> {
    tol.validate()?;
    if time_frame.is_empty() {
        return Err("Time frame must be non-empty".into());
    }
    let n = time_frame.len();
    if spectrum.len() != n {
        return Err(format!(
            "Complete spectrum length ({}) must match time frame length ({n})",
            spectrum.len(),
        )
        .into());
    }
    let denom = validate_denom(n, forward_scale)?;
    let time_energy =
        complex_energy_f64(time_frame, "Time frame contains non-finite complex sample")?;
    let spectral_sum = complex_energy_f64(spectrum, "Spectrum contains non-finite complex bin")?;

    build_report(
        n,
        forward_scale,
        false,
        time_energy,
        spectral_sum / denom,
        tol,
    )
}

/// Verify Parseval / Rayleigh energy consistency for a real input frame
/// `x[0..N]` and its complete complex spectrum `X[0..N]`.
pub fn verify_real_full_spectrum_energy(
    time_frame: &[f64],
    spectrum: &[Complex<f64>],
    forward_scale: f64,
    tol: FftToleranceConfig,
) -> Result<EnergyConsistencyReport> {
    tol.validate()?;
    if time_frame.is_empty() {
        return Err("Time frame must be non-empty".into());
    }
    let n = time_frame.len();
    if spectrum.len() != n {
        return Err(format!(
            "Complete spectrum length ({}) must match time frame length ({n})",
            spectrum.len(),
        )
        .into());
    }
    let denom = validate_denom(n, forward_scale)?;
    let time_energy = real_energy_f64(time_frame, "Time frame contains non-finite real sample")?;
    let spectral_sum = complex_energy_f64(spectrum, "Spectrum contains non-finite complex bin")?;

    build_report(
        n,
        forward_scale,
        false,
        time_energy,
        spectral_sum / denom,
        tol,
    )
}

/// Verify Parseval / Rayleigh energy consistency for a real input frame
/// `x[0..N]` and its one-sided non-negative-frequency spectrum `X[0..=N/2]`.
///
/// Conjugate-symmetry weighting: `k = 0` and (for even `N`) `k = N/2` have weight 1;
/// all other bins have weight 2 (odd-length transforms have no Nyquist singleton bin).
pub fn verify_real_one_sided_spectrum_energy(
    time_frame: &[f64],
    one_sided: &[Complex<f64>],
    forward_scale: f64,
    tol: FftToleranceConfig,
) -> Result<EnergyConsistencyReport> {
    tol.validate()?;
    if time_frame.is_empty() {
        return Err("Time frame must be non-empty".into());
    }
    let n = time_frame.len();
    let expected_bins = n / 2 + 1;
    if one_sided.len() != expected_bins {
        return Err(format!(
            "One-sided spectrum length ({}) must equal N/2 + 1 ({expected_bins}) for N = {n}",
            one_sided.len()
        )
        .into());
    }
    let denom = validate_denom(n, forward_scale)?;
    let time_energy = real_energy_f64(time_frame, "Time frame contains non-finite real sample")?;
    let spectral_sum = one_sided_energy_f64(
        one_sided,
        n,
        "One-sided spectrum contains non-finite complex bin",
    )?;

    build_report(
        n,
        forward_scale,
        true,
        time_energy,
        spectral_sum / denom,
        tol,
    )
}

/// Diagnostic helper matching SpectraCraft's `f32` `rustfft` forward path
/// (as used in `lib.rs` and `capture.rs`).
///
/// Fuses time-domain energy and full/one-sided spectral energy into single passes
/// without allocating intermediate `Vec<f64>` or `Vec<Complex<f64>>` copies.
pub fn check_rustfft_real_frame_f32(
    windowed_padded_frame: &[f32],
) -> Result<(EnergyConsistencyReport, EnergyConsistencyReport)> {
    if windowed_padded_frame.is_empty() {
        return Err("Analysis frame must be non-empty".into());
    }
    let n = windowed_padded_frame.len();
    let mut time_acc = NeumaierAcc::default();
    let mut buf = Vec::with_capacity(n);
    for &s in windowed_padded_frame {
        if !s.is_finite() {
            return Err("Analysis frame contains non-finite f32 sample".into());
        }
        let sd = f64::from(s);
        time_acc.add(sd * sd);
        buf.push(Complex::new(s, 0.0));
    }

    let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
    fft.process(&mut buf);

    let nyquist_bin = n / 2;
    let is_even = n.is_multiple_of(2);
    let mut full_acc = NeumaierAcc::default();
    let mut one_sided_acc = NeumaierAcc::default();
    for (k, z) in buf.iter().enumerate() {
        let re = f64::from(z.re);
        let im = f64::from(z.im);
        if !re.is_finite() || !im.is_finite() {
            return Err("Spectrum contains non-finite complex bin".into());
        }
        let mag2 = re * re + im * im;
        full_acc.add(mag2);
        if k <= nyquist_bin {
            let weight = if k == 0 || (is_even && k == nyquist_bin) {
                1.0
            } else {
                2.0
            };
            one_sided_acc.add(weight * mag2);
        }
    }

    let time_energy = time_acc.finish();
    let denom = n as f64;
    let tol = FftToleranceConfig::for_f32_fft(n);
    let full = build_report(
        n,
        RUSTFFT_FORWARD_SCALE,
        false,
        time_energy,
        full_acc.finish() / denom,
        tol,
    )?;
    let one_sided = build_report(
        n,
        RUSTFFT_FORWARD_SCALE,
        true,
        time_energy,
        one_sided_acc.finish() / denom,
        tol,
    )?;
    Ok((full, one_sided))
}

/// Diagnostic helper matching SpectraCraft's `f64` `rustfft` forward path
/// (as used in `shape_profile.rs`, `translation_layer.rs`, `edge.rs`, and `acoustics_physics.rs`).
pub fn check_rustfft_real_frame_f64(
    windowed_padded_frame: &[f64],
) -> Result<(EnergyConsistencyReport, EnergyConsistencyReport)> {
    if windowed_padded_frame.is_empty() {
        return Err("Analysis frame must be non-empty".into());
    }
    let n = windowed_padded_frame.len();
    let mut time_acc = NeumaierAcc::default();
    let mut buf = Vec::with_capacity(n);
    for &s in windowed_padded_frame {
        if !s.is_finite() {
            return Err("Analysis frame contains non-finite f64 sample".into());
        }
        time_acc.add(s * s);
        buf.push(Complex::new(s, 0.0));
    }

    let fft = FftPlanner::<f64>::new().plan_fft_forward(n);
    fft.process(&mut buf);

    let nyquist_bin = n / 2;
    let is_even = n.is_multiple_of(2);
    let mut full_acc = NeumaierAcc::default();
    let mut one_sided_acc = NeumaierAcc::default();
    for (k, z) in buf.iter().enumerate() {
        if !z.re.is_finite() || !z.im.is_finite() {
            return Err("Spectrum contains non-finite complex bin".into());
        }
        let mag2 = z.re * z.re + z.im * z.im;
        full_acc.add(mag2);
        if k <= nyquist_bin {
            let weight = if k == 0 || (is_even && k == nyquist_bin) {
                1.0
            } else {
                2.0
            };
            one_sided_acc.add(weight * mag2);
        }
    }

    let time_energy = time_acc.finish();
    let denom = n as f64;
    let tol = FftToleranceConfig::for_f64_fft(n);
    let full = build_report(
        n,
        RUSTFFT_FORWARD_SCALE,
        false,
        time_energy,
        full_acc.finish() / denom,
        tol,
    )?;
    let one_sided = build_report(
        n,
        RUSTFFT_FORWARD_SCALE,
        true,
        time_energy,
        one_sided_acc.finish() / denom,
        tol,
    )?;
    Ok((full, one_sided))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn run_fft_f64(frame: &[f64]) -> Vec<Complex<f64>> {
        let fft = FftPlanner::<f64>::new().plan_fft_forward(frame.len());
        let mut buf: Vec<Complex<f64>> = frame.iter().map(|&x| Complex::new(x, 0.0)).collect();
        fft.process(&mut buf);
        buf
    }

    fn hann_window(len: usize) -> Vec<f64> {
        (0..len)
            .map(|i| 0.5 - 0.5 * (TAU * i as f64 / len as f64).cos())
            .collect()
    }

    fn deterministic_noise(len: usize, mut seed: u64) -> Vec<f64> {
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let unit = (seed >> 11) as f64 / ((1_u64 << 53) as f64);
            out.push(unit * 2.0 - 1.0);
        }
        out
    }

    #[test]
    fn provenance_and_research_status_are_honest_and_distinct() {
        let parseval = parseval_energy_source();
        assert_eq!(parseval.label, FFT_ENERGY_CONSISTENCY_LABEL);
        assert_eq!(parseval.origin, MathematicalOrigin::EstablishedIdentity);

        let record = alpoge_claude_rh_investigation_record();
        assert_eq!(record.status, ResearchStatus::NoApplicableMethod);
        assert!(!record.production_audio_modified);
        assert!(record.primary_sources.len() >= 5);

        let method = NoApplicableRhEvidenceMethod::default();
        let proposal = method.propose(&[0.0_f32; 16]).unwrap();
        assert!(matches!(proposal, EvidenceProposal::Abstained { .. }));
    }

    #[test]
    fn silence_passes_without_division_by_zero() {
        for n in [16_usize, 17, 256, 255] {
            let x = vec![0.0_f64; n];
            let (full, one_sided) = check_rustfft_real_frame_f64(&x).unwrap();
            for r in [&full, &one_sided] {
                assert!(r.passes);
                assert_eq!(r.time_energy, 0.0);
                assert_eq!(r.spectral_energy, 0.0);
                assert_eq!(r.absolute_error, 0.0);
                assert_eq!(r.relative_error, None);
                assert_eq!(r.label, FFT_ENERGY_CONSISTENCY_LABEL);
                assert_eq!(r.origin, MathematicalOrigin::EstablishedIdentity);
            }
        }
    }

    #[test]
    fn unit_impulse_and_constant_signal_conserve_energy_even_and_odd() {
        for n in [1_usize, 2, 17, 105, 255, 256] {
            let mut impulse = vec![0.0_f64; n];
            impulse[n / 3] = 1.0;
            let (full, one_sided) = check_rustfft_real_frame_f64(&impulse).unwrap();
            assert!(full.passes, "impulse full N={n}: {full:?}");
            assert!(one_sided.passes, "impulse one-sided N={n}: {one_sided:?}");
            assert!((full.time_energy - 1.0).abs() < 1e-15);

            let constant = vec![0.375_f64; n];
            let (full_dc, one_sided_dc) = check_rustfft_real_frame_f64(&constant).unwrap();
            assert!(full_dc.passes, "constant full N={n}: {full_dc:?}");
            assert!(
                one_sided_dc.passes,
                "constant one-sided N={n}: {one_sided_dc:?}"
            );
        }
    }

    #[test]
    fn bin_centered_and_non_bin_centered_sinusoids_and_two_tones_pass() {
        for n in [256_usize, 255] {
            let centered: Vec<f64> = (0..n)
                .map(|i| 0.8 * (TAU * 13.0 * i as f64 / n as f64).sin())
                .collect();
            let non_centered: Vec<f64> = (0..n)
                .map(|i| 0.8 * (TAU * 13.37 * i as f64 / n as f64 + 0.4).sin())
                .collect();
            let two_tones: Vec<f64> = (0..n)
                .map(|i| {
                    0.55 * (TAU * 7.0 * i as f64 / n as f64).sin()
                        + 0.35 * (TAU * 29.42 * i as f64 / n as f64 + 1.1).cos()
                })
                .collect();

            for sig in [&centered, &non_centered, &two_tones] {
                let (full, one_sided) = check_rustfft_real_frame_f64(sig).unwrap();
                assert!(full.passes, "N={n}: {full:?}");
                assert!(one_sided.passes, "N={n}: {one_sided:?}");
            }
        }
    }

    #[test]
    fn deterministic_noise_passes_for_f64_and_f32_even_and_odd() {
        for n in [105_usize, 255, 256, 2048] {
            let noise_f64 = deterministic_noise(n, 0x517cc1b727220a95 ^ (n as u64));
            let (full, one_sided) = check_rustfft_real_frame_f64(&noise_f64).unwrap();
            assert!(full.passes, "f64 noise full N={n}: {full:?}");
            assert!(one_sided.passes, "f64 noise one-sided N={n}: {one_sided:?}");

            let noise_f32: Vec<f32> = noise_f64.iter().map(|&v| v as f32).collect();
            let (full32, one32) = check_rustfft_real_frame_f32(&noise_f32).unwrap();
            assert!(full32.passes, "f32 noise full N={n}: {full32:?}");
            assert!(one32.passes, "f32 noise one-sided N={n}: {one32:?}");
        }
    }

    #[test]
    fn windowed_and_zero_padded_frames_pass_and_unwindowed_mismatch_fails() {
        let raw = deterministic_noise(192, 0x9e3779b97f4a7c15);
        let win = hann_window(192);
        let windowed: Vec<f64> = raw.iter().zip(&win).map(|(s, w)| s * w).collect();

        for fft_len in [256_usize, 255] {
            let mut padded = vec![0.0_f64; fft_len];
            padded[..windowed.len()].copy_from_slice(&windowed);
            let (full, one_sided) = check_rustfft_real_frame_f64(&padded).unwrap();
            assert!(full.passes, "windowed+padded full N={fft_len}: {full:?}");
            assert!(
                one_sided.passes,
                "windowed+padded one-sided N={fft_len}: {one_sided:?}"
            );

            // Comparing the UNWINDOWED raw padded frame against the spectrum of the
            // WINDOWED frame must fail — proving Parseval validates the exact FFT input.
            let mut raw_padded = vec![0.0_f64; fft_len];
            raw_padded[..raw.len()].copy_from_slice(&raw);
            let spec = run_fft_f64(&padded);
            let tol = FftToleranceConfig::for_f64_fft(fft_len);
            let mismatch =
                verify_real_full_spectrum_energy(&raw_padded, &spec, RUSTFFT_FORWARD_SCALE, tol)
                    .unwrap();
            assert!(
                !mismatch.passes,
                "unwindowed frame vs windowed spectrum must fail"
            );
        }
    }

    #[test]
    fn complex_input_complete_spectrum_and_general_forward_scale_pass() {
        let n = 105_usize;
        let real_part = deterministic_noise(n, 0x1234_5678_9abc_def0);
        let imag_part = deterministic_noise(n, 0xfedc_ba98_7654_3210);
        let time_c: Vec<Complex<f64>> = real_part
            .into_iter()
            .zip(imag_part)
            .map(|(re, im)| Complex::new(re, im))
            .collect();

        let fft = FftPlanner::<f64>::new().plan_fft_forward(n);
        let mut spec = time_c.clone();
        fft.process(&mut spec);

        // Verify unitary-scaled spectrum (a = 1 / sqrt(N))
        let unitary_scale = 1.0 / (n as f64).sqrt();
        let unitary_spec: Vec<Complex<f64>> = spec.iter().map(|&z| z * unitary_scale).collect();
        let tol = FftToleranceConfig::for_f64_fft(n);
        let report =
            verify_complex_spectrum_energy(&time_c, &unitary_spec, unitary_scale, tol).unwrap();
        assert!(report.passes, "unitary complex spectrum failed: {report:?}");
    }

    #[test]
    fn odd_and_even_nyquist_edge_cases_are_handled_distinctly() {
        // Pure Nyquist alternating signal (+1, -1, +1, -1, ...) for even N has ALL energy in bin N/2.
        let n_even = 64_usize;
        let nyquist_sig: Vec<f64> = (0..n_even)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let (full_e, one_e) = check_rustfft_real_frame_f64(&nyquist_sig).unwrap();
        assert!(full_e.passes);
        assert!(one_e.passes);
        assert!((one_e.spectral_energy - n_even as f64).abs() < 1e-11);

        // Highest-frequency cosine for odd N puts energy in bin (N-1)/2 AND its conjugate (N+1)/2,
        // so one-sided bin (N-1)/2 MUST be weighted by 2.0 (NOT 1.0).
        let n_odd = 65_usize;
        let k_top = (n_odd - 1) / 2;
        let top_sig: Vec<f64> = (0..n_odd)
            .map(|i| (TAU * (k_top as f64) * (i as f64) / (n_odd as f64)).cos())
            .collect();
        let (full_o, one_o) = check_rustfft_real_frame_f64(&top_sig).unwrap();
        assert!(full_o.passes);
        assert!(one_o.passes);
    }

    #[test]
    fn deliberate_normalization_errors_must_fail() {
        let n = 256_usize;
        let x = deterministic_noise(n, 0xdead_beef_cafe_babe);
        let spec = run_fft_f64(&x);
        let tol = FftToleranceConfig::for_f64_fft(n);

        // Wrong forward_scale: claiming unitary (1/sqrt(N)) for unnormalized rustfft output.
        let wrong_unitary =
            verify_real_full_spectrum_energy(&x, &spec, 1.0 / (n as f64).sqrt(), tol).unwrap();
        assert!(!wrong_unitary.passes);

        // Wrong forward_scale: claiming 1/N for unnormalized rustfft output.
        let wrong_inv_n =
            verify_real_full_spectrum_energy(&x, &spec, 1.0 / (n as f64), tol).unwrap();
        assert!(!wrong_inv_n.passes);

        // Deliberate 1% spectrum scaling corruption.
        let corrupted: Vec<Complex<f64>> = spec.iter().map(|&z| z * 1.01).collect();
        let scaled_err =
            verify_real_full_spectrum_energy(&x, &corrupted, RUSTFFT_FORWARD_SCALE, tol).unwrap();
        assert!(!scaled_err.passes);

        // Deliberate missing factor of 2 on positive one-sided bins (scaling interior bins by 1/sqrt(2)).
        let mut underweighted = spec[..=n / 2].to_vec();
        for z in &mut underweighted[1..n / 2] {
            *z *= std::f64::consts::FRAC_1_SQRT_2;
        }
        let one_sided_err =
            verify_real_one_sided_spectrum_energy(&x, &underweighted, RUSTFFT_FORWARD_SCALE, tol)
                .unwrap();
        assert!(!one_sided_err.passes);
    }

    #[test]
    fn invalid_inputs_and_parameters_are_rejected() {
        let tol = FftToleranceConfig::for_f64_fft(16);
        let zeros = vec![0.0_f64; 16];
        let spec = vec![Complex::new(0.0_f64, 0.0); 16];
        let one_sided = vec![Complex::new(0.0_f64, 0.0); 9];

        // Empty frame
        assert!(verify_real_full_spectrum_energy(&[], &[], 1.0, tol).is_err());
        // Non-finite sample
        let mut bad_x = zeros.clone();
        bad_x[3] = f64::NAN;
        assert!(verify_real_full_spectrum_energy(&bad_x, &spec, 1.0, tol).is_err());
        bad_x[3] = f64::INFINITY;
        assert!(verify_real_one_sided_spectrum_energy(&bad_x, &one_sided, 1.0, tol).is_err());
        // Non-finite spectrum
        let mut bad_s = spec.clone();
        bad_s[2] = Complex::new(0.0, f64::NEG_INFINITY);
        assert!(verify_real_full_spectrum_energy(&zeros, &bad_s, 1.0, tol).is_err());
        // Mismatched lengths
        assert!(verify_real_full_spectrum_energy(&zeros, &spec[..15], 1.0, tol).is_err());
        assert!(verify_real_one_sided_spectrum_energy(&zeros, &one_sided[..8], 1.0, tol).is_err());
        // Invalid forward scale
        assert!(verify_real_full_spectrum_energy(&zeros, &spec, 0.0, tol).is_err());
        assert!(verify_real_full_spectrum_energy(&zeros, &spec, -1.0, tol).is_err());
        assert!(verify_real_full_spectrum_energy(&zeros, &spec, f64::NAN, tol).is_err());
        // Invalid tolerances
        let neg_tol = FftToleranceConfig {
            absolute_tolerance: -1e-12,
            relative_tolerance: 1e-12,
        };
        assert!(verify_real_full_spectrum_energy(&zeros, &spec, 1.0, neg_tol).is_err());
        let nan_tol = FftToleranceConfig {
            absolute_tolerance: 1e-12,
            relative_tolerance: f64::NAN,
        };
        assert!(verify_real_full_spectrum_energy(&zeros, &spec, 1.0, nan_tol).is_err());
    }
}
