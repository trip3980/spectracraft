//! Acoustic calculators, formulas, and analytical estimators for SpectraCraft Capture Lab.
//!
//! References:
//! - AudioDesignSpace acoustic calculators (`audiodesignspace.com/tools` concepts)
//! - Leslie Gaston-Bird, *Math Fundamentals for Audio* (distance/rate/time, logarithms for dB,
//!   wavelength/frequency, inverse-square propagation, trigonometry for polar pickup & angles)
//!
//! Important architectural boundary:
//! - These formulas support estimation, display, and verification.
//! - They NEVER replace the geometric 6-band image-source capture engine (`capture_paths_6band`).
//! - Sabine RT60 and rectangular axial room modes are clearly labeled informational estimates.
//! - All internal math uses SI metres, seconds, Hz, and radians; zero runtime network access.

use crate::studio_catalog::{
    speed_of_sound_celsius, MicEntry, Pattern, RoomPreset, SourceEntry, StudioRig, SurfaceEntry,
    BANDS_HZ, FACE_LABELS,
};
use serde::Serialize;

/// Standard room-acoustics speed of sound in dry air at 20 °C (m/s).
pub const STANDARD_SPEED_OF_SOUND_MPS: f64 = 343.0;

/// Metric Sabine reverberation constant: `RT60 ≈ 0.161 * V / A` (V in m³, A in metric sabins m²).
pub const SABINE_CONSTANT_METRIC: f64 = 0.161;

/// Physical propagation delay in seconds: `distance_m / speed_mps`.
pub fn distance_to_delay_s(distance_m: f64, speed_mps: f64) -> f64 {
    if !distance_m.is_finite() || !speed_mps.is_finite() || speed_mps <= 1.0 {
        return 0.0;
    }
    distance_m.max(0.0) / speed_mps
}

/// Physical propagation delay in milliseconds at a given speed of sound.
pub fn distance_to_delay_ms(distance_m: f64, speed_mps: f64) -> f64 {
    distance_to_delay_s(distance_m, speed_mps) * 1000.0
}

/// Wavelength in metres: `λ = c / f`.
pub fn wavelength_m(freq_hz: f64, speed_mps: f64) -> Option<f64> {
    if !freq_hz.is_finite() || !speed_mps.is_finite() || freq_hz <= 0.0 || speed_mps <= 1.0 {
        return None;
    }
    Some(speed_mps / freq_hz)
}

/// Frequency in Hz from wavelength in metres: `f = c / λ`.
pub fn frequency_from_wavelength_hz(lambda_m: f64, speed_mps: f64) -> Option<f64> {
    if !lambda_m.is_finite() || !speed_mps.is_finite() || lambda_m <= 0.0 || speed_mps <= 1.0 {
        return None;
    }
    Some(speed_mps / lambda_m)
}

/// Decibel conversion for acoustic pressure / voltage ratios: `20 * log10(|ratio|)`.
pub fn pressure_ratio_to_db(ratio: f64) -> Option<f64> {
    let a = ratio.abs();
    if !a.is_finite() || a <= 1e-12 {
        return None;
    }
    Some(20.0 * a.log10())
}

/// Decibel conversion for acoustic intensity / power ratios: `10 * log10(ratio)`.
pub fn intensity_ratio_to_db(ratio: f64) -> Option<f64> {
    if !ratio.is_finite() || ratio <= 1e-15 {
        return None;
    }
    Some(10.0 * ratio.log10())
}

/// Free-field inverse-square pressure ratio `r_ref / r` (intensity scales as `(r_ref / r)²`).
pub fn inverse_square_pressure_ratio(distance_m: f64, reference_m: f64) -> f64 {
    if !distance_m.is_finite() || !reference_m.is_finite() || distance_m <= 1e-6 || reference_m <= 0.0
    {
        return 1.0;
    }
    reference_m / distance_m
}

/// Free-field inverse-square SPL / level change in dB relative to `reference_m`:
/// `ΔL = 20 * log10(r_ref / r) = 10 * log10((r_ref / r)²)`.
/// Doubling distance yields `-6.0206 dB`.
pub fn inverse_square_level_db(distance_m: f64, reference_m: f64) -> f64 {
    let ratio = inverse_square_pressure_ratio(distance_m, reference_m);
    20.0 * ratio.log10()
}

/// Clamp raw published absorption to passive effective absorption `[0.0, 1.0]`.
pub fn effective_absorption(raw_alpha: f64) -> f64 {
    if raw_alpha.is_finite() {
        raw_alpha.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Passive reflection pressure multiplier: `R = sqrt(1.0 - effective_absorption)`.
pub fn absorption_to_pressure_multiplier(effective_alpha: f64) -> f64 {
    let a = effective_absorption(effective_alpha);
    (1.0 - a).sqrt()
}

/// Estimated Sabine reverberation time in seconds: `RT60 ≈ 0.161 * V / A`.
/// Returns `None` when volume or total absorption is non-positive / non-finite.
pub fn sabine_rt60_s(volume_m3: f64, total_absorption_sabins_m2: f64) -> Option<f64> {
    if !volume_m3.is_finite()
        || !total_absorption_sabins_m2.is_finite()
        || volume_m3 <= 0.0
        || total_absorption_sabins_m2 <= 1e-6
    {
        return None;
    }
    Some(SABINE_CONSTANT_METRIC * volume_m3 / total_absorption_sabins_m2)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AxialRoomMode {
    pub axis: String,
    pub axis_key: String,
    pub dimension_m: f64,
    pub harmonic: u32,
    pub freq_hz: f64,
    pub wavelength_m: f64,
}

/// Compute rectangular-room axial standing-wave mode estimates `f_n = n * c / (2 * L)`.
/// Sorted ascending by frequency. Informational estimate only for enclosed rectangular rooms.
pub fn axial_room_modes(
    dimensions_m: [f64; 3],
    speed_mps: f64,
    max_harmonic: u32,
) -> Vec<AxialRoomMode> {
    if !speed_mps.is_finite() || speed_mps <= 1.0 || max_harmonic == 0 {
        return Vec::new();
    }
    let axes = [
        ("Width (X)", "X", dimensions_m[0]),
        ("Length (Y)", "Y", dimensions_m[1]),
        ("Height (Z)", "Z", dimensions_m[2]),
    ];
    let mut modes = Vec::new();
    for (axis_label, axis_key, len_m) in axes {
        if !len_m.is_finite() || len_m <= 0.05 {
            continue;
        }
        for n in 1..=max_harmonic {
            let freq_hz = f64::from(n) * speed_mps / (2.0 * len_m);
            let wl = (2.0 * len_m) / f64::from(n);
            modes.push(AxialRoomMode {
                axis: axis_label.to_owned(),
                axis_key: axis_key.to_owned(),
                dimension_m: len_m,
                harmonic: n,
                freq_hz,
                wavelength_m: wl,
            });
        }
    }
    modes.sort_by(|a, b| a.freq_hz.total_cmp(&b.freq_hz));
    modes
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MicAngularAnalysis {
    pub distance_m: f64,
    pub direct_delay_ms: f64,
    pub inverse_square_pressure_vs_1m: f64,
    pub inverse_square_db_vs_1m: f64,
    pub aim_unit: [f64; 3],
    pub heading_z_deg: f64,
    pub tilt_x_deg: f64,
    pub roll_y_deg: f64,
    pub follow_source: bool,
    pub off_axis_deg: f64,
    pub cos_theta: f64,
    pub pattern_label: String,
    pub pattern_formula: String,
    pub polar_gain_linear: f64,
    pub polar_gain_db: Option<f64>,
    pub source_radiation_label: String,
    pub source_radiation_gain: f64,
    pub combined_direct_gain: f64,
}

fn pattern_formula_text(pattern: Pattern) -> &'static str {
    match pattern {
        Pattern::Omni => "1.0 (omnidirectional)",
        Pattern::WideCardioid => "0.70 + 0.30 · cos(θ)",
        Pattern::Cardioid => "0.50 + 0.50 · cos(θ)",
        Pattern::Supercardioid => "0.37 + 0.63 · cos(θ)",
        Pattern::Hypercardioid => "0.25 + 0.75 · cos(θ)",
        Pattern::Figure8 => "cos(θ) (bidirectional)",
    }
}

/// Evaluate exact direct-path trigonometry, propagation delay, inverse-square attenuation,
/// and analytical polar weighting for a source/mic pair.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_direct_mic_trigonometry(
    source_pos_m: [f64; 3],
    mic_pos_m: [f64; 3],
    aim_unit: [f64; 3],
    heading_z_deg: f64,
    tilt_x_deg: f64,
    roll_y_deg: f64,
    follow_source: bool,
    pattern: Pattern,
    source_radiation_label: &str,
    source_radiation_gain: f64,
    speed_mps: f64,
) -> MicAngularAnalysis {
    let off = [
        mic_pos_m[0] - source_pos_m[0],
        mic_pos_m[1] - source_pos_m[1],
        mic_pos_m[2] - source_pos_m[2],
    ];
    let dist = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2])
        .sqrt()
        .max(0.20);
    // Unit vector pointing from mic toward source (incoming direct wave direction)
    let to_src = [-off[0] / dist, -off[1] / dist, -off[2] / dist];
    let aim_len = (aim_unit[0] * aim_unit[0] + aim_unit[1] * aim_unit[1] + aim_unit[2] * aim_unit[2])
        .sqrt()
        .max(1e-9);
    let aim_norm = [
        aim_unit[0] / aim_len,
        aim_unit[1] / aim_len,
        aim_unit[2] / aim_len,
    ];
    let cos_theta = (to_src[0] * aim_norm[0] + to_src[1] * aim_norm[1] + to_src[2] * aim_norm[2])
        .clamp(-1.0, 1.0);
    let off_axis_deg = cos_theta.acos().to_degrees();
    let polar_gain_linear = pattern.pressure(cos_theta);
    let polar_gain_db = pressure_ratio_to_db(polar_gain_linear);
    let combined_direct_gain = (source_radiation_gain * polar_gain_linear).clamp(-1.0, 1.0);

    MicAngularAnalysis {
        distance_m: dist,
        direct_delay_ms: distance_to_delay_ms(dist, speed_mps),
        inverse_square_pressure_vs_1m: inverse_square_pressure_ratio(dist, 1.0),
        inverse_square_db_vs_1m: inverse_square_level_db(dist, 1.0),
        aim_unit: aim_norm,
        heading_z_deg,
        tilt_x_deg,
        roll_y_deg,
        follow_source,
        off_axis_deg,
        cos_theta,
        pattern_label: pattern.label().to_owned(),
        pattern_formula: pattern_formula_text(pattern).to_owned(),
        polar_gain_linear,
        polar_gain_db,
        source_radiation_label: source_radiation_label.to_owned(),
        source_radiation_gain,
        combined_direct_gain,
    }
}

/// Standard ORTF stereo pair capsule spacing in metres (17 cm).
pub const ORTF_SPACING_M: f64 = 0.17;
/// Standard ORTF stereo pair included angle in degrees (110° total, ±55° per capsule).
pub const ORTF_INCLUDED_ANGLE_DEG: f64 = 110.0;
/// Recommended X/Y coincident included angle range (90°–135°).
pub const XY_MIN_ANGLE_DEG: f64 = 90.0;
pub const XY_MAX_ANGLE_DEG: f64 = 135.0;

/// 3:1 rule minimum distance for an additional mic from the source: `3 × nearest_mic_distance`.
/// Guidance only — not a hard constraint.
pub fn three_to_one_min_distance_m(nearest_source_distance_m: f64) -> f64 {
    if !nearest_source_distance_m.is_finite() || nearest_source_distance_m <= 0.0 {
        return 0.0;
    }
    nearest_source_distance_m * 3.0
}

/// Free-field level difference estimate between two mic distances from the same source:
/// `ΔL = 20 * log10(d_far / d_near)` dB (at 3:1 ratio, `20 * log10(3) ≈ 9.542 dB`).
pub fn distance_ratio_level_diff_db(d_other_m: f64, d_ref_m: f64) -> f64 {
    if !d_other_m.is_finite() || !d_ref_m.is_finite() || d_other_m <= 1e-6 || d_ref_m <= 1e-6 {
        return 0.0;
    }
    20.0 * (d_other_m / d_ref_m).log10()
}

/// Spaced-pair stereo spacing guidance: `spacing ≈ source_distance * tan(desired_width / 2)`,
/// clamped to `[0.15 m, 3.0 m]` for practical studio placement.
pub fn spaced_pair_spacing_m(source_distance_m: f64, desired_width_deg: f64) -> f64 {
    if !source_distance_m.is_finite() || source_distance_m <= 0.0 {
        return 0.5;
    }
    let w_deg = if desired_width_deg.is_finite() {
        desired_width_deg.clamp(20.0, 120.0)
    } else {
        60.0
    };
    let raw = source_distance_m * (w_deg * 0.5).to_radians().tan();
    raw.clamp(0.15, 3.0)
}

/// Clamp an X/Y coincident included angle to the standard 90°–135° recommendation.
pub fn xy_clamped_angle_deg(angle_deg: f64) -> f64 {
    if !angle_deg.is_finite() {
        return XY_MIN_ANGLE_DEG;
    }
    angle_deg.clamp(XY_MIN_ANGLE_DEG, XY_MAX_ANGLE_DEG)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MicSlotPlacementRow {
    pub slot_index: usize,
    pub label: String,
    pub enabled: bool,
    pub source_distance_m: f64,
    pub direct_delay_ms: f64,
    pub distance_from_nearest_mic_m: f64,
    pub ratio_to_nearest_source_dist: f64,
    pub level_diff_vs_nearest_db: f64,
    pub delay_offset_vs_nearest_ms: f64,
    /// `None` for the nearest reference mic or single-mic setups; `Some(true)` if `>= 3×` nearest distance.
    pub meets_three_to_one: Option<bool>,
    pub shortfall_m: f64,
    pub status_label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StereoPairGuidance {
    pub ortf_spacing_m: f64,
    pub ortf_included_angle_deg: f64,
    pub xy_default_angle_deg: f64,
    pub xy_min_angle_deg: f64,
    pub xy_max_angle_deg: f64,
    pub spaced_pair_width_deg: f64,
    pub spaced_pair_suggested_spacing_m: f64,
    pub guidance_note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MultiMicPlacementGuidance {
    pub active_mic_count: usize,
    pub multi_mic_active: bool,
    pub nearest_mic_index: usize,
    pub nearest_mic_label: String,
    pub nearest_source_distance_m: f64,
    pub three_to_one_min_distance_m: f64,
    pub three_to_one_reference_level_diff_db: f64,
    pub slots: Vec<MicSlotPlacementRow>,
    pub stereo_helpers: StereoPairGuidance,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FaceAbsorptionContribution {
    pub face_index: usize,
    pub face_label: String,
    pub active: bool,
    pub surface_index: usize,
    pub surface_name: String,
    pub area_m2: f64,
    pub raw_absorption_6band: [f64; 6],
    pub effective_absorption_6band: [f64; 6],
    pub pressure_reflectance_6band: [f64; 6],
    pub mean_effective_alpha: f64,
    pub sabins_6band_m2: [f64; 6],
    pub mean_sabins_m2: f64,
    pub share_of_total_absorption_pct: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StudioAcousticAnalysis {
    pub speed_of_sound_mps: f64,
    pub temperature_c: f64,
    pub room_preset: String,
    pub is_enclosed_room: bool,
    pub dimensions_m: [f64; 3],
    pub volume_m3: f64,
    pub total_surface_area_m2: f64,
    pub bands_hz: [f64; 6],
    pub total_absorption_sabins_6band_m2: [f64; 6],
    pub mean_total_absorption_sabins_m2: f64,
    pub sabine_rt60_6band_s: [Option<f64>; 6],
    pub sabine_rt60_mid_s: Option<f64>,
    pub sabine_rt60_overall_s: Option<f64>,
    pub axial_modes: Vec<AxialRoomMode>,
    pub face_contributions: Vec<FaceAbsorptionContribution>,
    pub direct_path: MicAngularAnalysis,
    pub placement_guidance: MultiMicPlacementGuidance,
    pub estimate_disclaimer: String,
}

/// Compute the full, honest, single-source-of-truth acoustic calculator & estimator summary
/// directly from a `StudioRig` and the offline catalogs.
pub fn analyze_studio_rig(
    rig: &StudioRig,
    sources: &[SourceEntry],
    mics: &[MicEntry],
    surfaces: &[SurfaceEntry],
) -> StudioAcousticAnalysis {
    let mut r = rig.clone();
    r.clamp_all();

    let c = speed_of_sound_celsius(f64::from(r.temperature_c));
    let [w, d, h] = r.dimensions_m.map(f64::from);
    let volume_m3 = w * d * h;
    // Face order matches studio_catalog::FACE_LABELS:
    // 0: Left (-X) = d*h, 1: Right (+X) = d*h, 2: Back (-Y) = w*h, 3: Front (+Y) = w*h,
    // 4: Floor (-Z) = w*d, 5: Ceiling (+Z) = w*d.
    let face_areas_m2 = [d * h, d * h, w * h, w * h, w * d, w * d];
    let total_surface_area_m2: f64 = face_areas_m2.iter().sum();

    let is_enclosed_room = matches!(
        r.room_preset,
        RoomPreset::Small | RoomPreset::Medium | RoomPreset::Large
    );

    let preset_str = match r.room_preset {
        RoomPreset::Small => "small",
        RoomPreset::Medium => "medium",
        RoomPreset::Large => "large",
        RoomPreset::Open => "open",
        RoomPreset::Direct => "direct",
    };

    let mut total_sabins_6 = [0.0_f64; 6];
    let mut raw_faces: Vec<FaceAbsorptionContribution> = Vec::with_capacity(6);

    for f in 0..6 {
        let active = match r.room_preset {
            RoomPreset::Direct => false,
            RoomPreset::Open => f == 4,
            _ => true,
        };
        let surf_idx = r.surface_faces[f].min(surfaces.len().saturating_sub(1));
        let surf = surfaces.get(surf_idx);
        let raw_6 = surf
            .map(|s| s.absorption_6band.map(f64::from))
            .unwrap_or([1.0; 6]);
        let eff_6 = raw_6.map(effective_absorption);
        let refl_6 = eff_6.map(absorption_to_pressure_multiplier);
        let area = face_areas_m2[f];
        let sabins_6 = if active {
            eff_6.map(|a| a * area)
        } else {
            [0.0; 6]
        };
        for b in 0..6 {
            total_sabins_6[b] += sabins_6[b];
        }
        let mean_eff = eff_6.iter().sum::<f64>() / 6.0;
        let mean_sabins = sabins_6.iter().sum::<f64>() / 6.0;
        raw_faces.push(FaceAbsorptionContribution {
            face_index: f,
            face_label: FACE_LABELS[f].to_owned(),
            active,
            surface_index: surf_idx,
            surface_name: surf.map(|s| s.name.clone()).unwrap_or_else(|| "Unknown".to_owned()),
            area_m2: area,
            raw_absorption_6band: raw_6,
            effective_absorption_6band: eff_6,
            pressure_reflectance_6band: refl_6,
            mean_effective_alpha: mean_eff,
            sabins_6band_m2: sabins_6,
            mean_sabins_m2: mean_sabins,
            share_of_total_absorption_pct: 0.0,
        });
    }

    let mean_total_sabins = total_sabins_6.iter().sum::<f64>() / 6.0;
    if mean_total_sabins > 1e-9 {
        for fc in &mut raw_faces {
            fc.share_of_total_absorption_pct = (fc.mean_sabins_m2 / mean_total_sabins) * 100.0;
        }
    }

    let sabine_rt60_6band_s = if is_enclosed_room {
        std::array::from_fn(|b| sabine_rt60_s(volume_m3, total_sabins_6[b]))
    } else {
        [None; 6]
    };

    // Standard mid-band Sabine RT60 estimate: average of 500 Hz (idx 2) and 1000 Hz (idx 3)
    let sabine_rt60_mid_s = match (sabine_rt60_6band_s[2], sabine_rt60_6band_s[3]) {
        (Some(a), Some(b)) => Some(0.5 * (a + b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };

    let sabine_rt60_overall_s = if is_enclosed_room {
        sabine_rt60_s(volume_m3, mean_total_sabins)
    } else {
        None
    };

    let axial_modes = if is_enclosed_room {
        let mut modes = axial_room_modes([w, d, h], c, 4);
        modes.truncate(12);
        modes
    } else {
        Vec::new()
    };

    // Active microphone direct-path trigonometry
    let src_pos = r.source_pos_m().map(f64::from);
    let slot = r.active_mic();
    let mic_pos = [
        src_pos[0] + f64::from(slot.offset_m[0]),
        src_pos[1] + f64::from(slot.offset_m[1]),
        src_pos[2] + f64::from(slot.offset_m[2]),
    ];
    let dist = (f64::from(slot.offset_m[0]).powi(2)
        + f64::from(slot.offset_m[1]).powi(2)
        + f64::from(slot.offset_m[2]).powi(2))
    .sqrt()
    .max(0.20);
    let src_rad = sources
        .get(r.source_index)
        .map(|s| s.radiation)
        .unwrap_or(crate::studio_catalog::SourceRadiation::Omni);
    let dir_y = f64::from(slot.offset_m[1]) / dist;
    let src_rad_gain = src_rad.gain(dir_y);
    let mic_pat = slot.effective_pattern(mics);
    let aim_unit = slot.aim.direction().map(f64::from);

    let direct_path = evaluate_direct_mic_trigonometry(
        src_pos,
        mic_pos,
        aim_unit,
        f64::from(slot.aim.heading_z_deg),
        f64::from(slot.aim.tilt_x_deg),
        f64::from(slot.aim.roll_y_deg),
        slot.aim.follow_source,
        mic_pat,
        src_rad.label(),
        src_rad_gain,
        c,
    );

    // Multi-mic placement guidance (3:1 rule + ORTF / X-Y / Spaced Pair helpers)
    let active_indices: Vec<usize> = r
        .mics
        .iter()
        .enumerate()
        .filter(|(_, m)| m.enabled)
        .map(|(i, _)| i)
        .collect();
    let active_mic_count = active_indices.len();
    let multi_mic_active = active_mic_count >= 2;

    let slot_dists: Vec<f64> = r
        .mics
        .iter()
        .map(|m| {
            (f64::from(m.offset_m[0]).powi(2)
                + f64::from(m.offset_m[1]).powi(2)
                + f64::from(m.offset_m[2]).powi(2))
            .sqrt()
            .max(0.20)
        })
        .collect();

    let nearest_mic_index = active_indices
        .iter()
        .copied()
        .min_by(|&a, &b| slot_dists[a].total_cmp(&slot_dists[b]))
        .unwrap_or(0);
    let nearest_source_distance_m = slot_dists.get(nearest_mic_index).copied().unwrap_or(dist);
    let nearest_mic_label = r
        .mics
        .get(nearest_mic_index)
        .map(|m| m.label.clone())
        .unwrap_or_else(|| "Mic 1".to_owned());
    let nearest_off = r
        .mics
        .get(nearest_mic_index)
        .map(|m| m.offset_m.map(f64::from))
        .unwrap_or([0.0, nearest_source_distance_m, 0.0]);
    let min_3to1_m = three_to_one_min_distance_m(nearest_source_distance_m);
    let ref_3to1_db = distance_ratio_level_diff_db(3.0, 1.0);
    let nearest_delay_ms = distance_to_delay_ms(nearest_source_distance_m, c);

    let mut placement_rows = Vec::with_capacity(r.mics.len());
    for (idx, m) in r.mics.iter().enumerate() {
        let d_src = slot_dists[idx];
        let delay_ms = distance_to_delay_ms(d_src, c);
        let m_off = m.offset_m.map(f64::from);
        let d_between = ((m_off[0] - nearest_off[0]).powi(2)
            + (m_off[1] - nearest_off[1]).powi(2)
            + (m_off[2] - nearest_off[2]).powi(2))
        .sqrt();
        let ratio = d_src / nearest_source_distance_m.max(1e-6);
        let level_diff_db = distance_ratio_level_diff_db(d_src, nearest_source_distance_m);
        let delay_offset_ms = delay_ms - nearest_delay_ms;

        let (meets, shortfall, status_label) = if !m.enabled {
            (None, 0.0, "Muted".to_owned())
        } else if !multi_mic_active || idx == nearest_mic_index {
            (None, 0.0, "Nearest Reference".to_owned())
        } else if d_src + 1e-4 >= min_3to1_m {
            (Some(true), 0.0, "Meets 3:1".to_owned())
        } else {
            (
                Some(false),
                (min_3to1_m - d_src).max(0.0),
                "Shortfall (< 3:1)".to_owned(),
            )
        };

        placement_rows.push(MicSlotPlacementRow {
            slot_index: idx,
            label: m.label.clone(),
            enabled: m.enabled,
            source_distance_m: d_src,
            direct_delay_ms: delay_ms,
            distance_from_nearest_mic_m: d_between,
            ratio_to_nearest_source_dist: ratio,
            level_diff_vs_nearest_db: level_diff_db,
            delay_offset_vs_nearest_ms: delay_offset_ms,
            meets_three_to_one: meets,
            shortfall_m: shortfall,
            status_label,
        });
    }

    let placement_guidance = MultiMicPlacementGuidance {
        active_mic_count,
        multi_mic_active,
        nearest_mic_index,
        nearest_mic_label,
        nearest_source_distance_m,
        three_to_one_min_distance_m: min_3to1_m,
        three_to_one_reference_level_diff_db: ref_3to1_db,
        slots: placement_rows,
        stereo_helpers: StereoPairGuidance {
            ortf_spacing_m: ORTF_SPACING_M,
            ortf_included_angle_deg: ORTF_INCLUDED_ANGLE_DEG,
            xy_default_angle_deg: XY_MIN_ANGLE_DEG,
            xy_min_angle_deg: XY_MIN_ANGLE_DEG,
            xy_max_angle_deg: XY_MAX_ANGLE_DEG,
            spaced_pair_width_deg: 60.0,
            spaced_pair_suggested_spacing_m: spaced_pair_spacing_m(nearest_source_distance_m, 60.0),
            guidance_note: "Starting-point placement guidance only — never a hard constraint. The geometric capture engine always renders from actual mic positions, aims, patterns, and room surfaces.".to_owned(),
        },
    };

    StudioAcousticAnalysis {
        speed_of_sound_mps: c,
        temperature_c: f64::from(r.temperature_c),
        room_preset: preset_str.to_owned(),
        is_enclosed_room,
        dimensions_m: [w, d, h],
        volume_m3,
        total_surface_area_m2,
        bands_hz: BANDS_HZ,
        total_absorption_sabins_6band_m2: total_sabins_6,
        mean_total_absorption_sabins_m2: mean_total_sabins,
        sabine_rt60_6band_s,
        sabine_rt60_mid_s,
        sabine_rt60_overall_s,
        axial_modes,
        face_contributions: raw_faces,
        direct_path,
        placement_guidance,
        estimate_disclaimer: "Informational estimates (Sabine RT60, axial room modes, inverse-square, wavelength, 3:1 guidance) derived from room geometry, clamped effective absorption, and mic aim. Rendered capture audio uses geometric 6-band image-source paths — never replaced by a synthetic RT60 tail.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::render_studio_rig;
    use crate::studio_catalog::{
        capture_paths_6band, mic_catalog, source_catalog, surface_catalog, Timing, Unit,
        FEET_TO_METRES,
    };

    #[test]
    fn two_feet_distance_delay_matches_1_777_ms_at_343_mps() {
        let two_ft_m = 2.0 * f64::from(FEET_TO_METRES); // 0.6096 m
        let delay_ms = distance_to_delay_ms(two_ft_m, STANDARD_SPEED_OF_SOUND_MPS);
        // 0.6096 / 343.0 * 1000 = 1.777259475... ms
        assert!(
            (delay_ms - 1.777259475).abs() < 1e-4,
            "expected ~1.777 ms at 2 ft & 343 m/s, got {delay_ms:.6} ms"
        );

        // Verify the default StudioRig (2 ft SM7B at 20°C = 343.0 m/s) matches this in both
        // analyze_studio_rig AND the geometric image-source capture engine!
        let rig = StudioRig::default();
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();
        let analysis = analyze_studio_rig(&rig, &sources, &mics, &surfaces);
        let plan = capture_paths_6band(&rig, &sources, &mics, &surfaces, 4, Timing::Physical);

        assert!((analysis.speed_of_sound_mps - 343.0).abs() < 1e-6);
        assert!((analysis.direct_path.direct_delay_ms - 1.777259).abs() < 0.005);
        assert!(
            (analysis.direct_path.direct_delay_ms - plan.direct_arrival_s * 1000.0).abs() < 1e-6,
            "calculator delay must match geometric engine delay"
        );
    }

    #[test]
    fn core_audio_math_formulas_match_reference_identities() {
        // Wavelength & frequency
        let wl_1k = wavelength_m(1000.0, 343.0).unwrap();
        assert!((wl_1k - 0.343).abs() < 1e-9);
        assert!((frequency_from_wavelength_hz(wl_1k, 343.0).unwrap() - 1000.0).abs() < 1e-9);

        // Inverse-square law: doubling distance drops level by 6.0206 dB
        let db_1m = inverse_square_level_db(1.0, 1.0);
        let db_2m = inverse_square_level_db(2.0, 1.0);
        let db_4m = inverse_square_level_db(4.0, 1.0);
        assert!(db_1m.abs() < 1e-9);
        assert!((db_2m - (-6.0205999)).abs() < 1e-5);
        assert!((db_4m - (-12.0411998)).abs() < 1e-5);

        // Absorption clamping & pressure multiplier
        assert_eq!(effective_absorption(1.05), 1.0);
        assert_eq!(effective_absorption(-0.2), 0.0);
        assert!((absorption_to_pressure_multiplier(1.05) - 0.0).abs() < 1e-12);
        assert!((absorption_to_pressure_multiplier(0.75) - 0.5).abs() < 1e-12);
        assert!((absorption_to_pressure_multiplier(0.0) - 1.0).abs() < 1e-12);

        // Sabine RT60: 100 m³ room with 16.1 sabins -> 1.000 s
        let rt60 = sabine_rt60_s(100.0, 16.1).unwrap();
        assert!((rt60 - 1.0).abs() < 1e-9);

        // Axial room modes: 3.43 m dimension -> 1st mode = 50 Hz, 2nd mode = 100 Hz
        let modes = axial_room_modes([3.43, 6.86, 3.43], 343.0, 2);
        assert!((modes[0].freq_hz - 25.0).abs() < 1e-9); // Y 1st mode
        assert!(modes.iter().any(|m| m.axis_key == "X" && m.harmonic == 1 && (m.freq_hz - 50.0).abs() < 1e-9));
        assert!(modes.iter().any(|m| m.axis_key == "X" && m.harmonic == 2 && (m.freq_hz - 100.0).abs() < 1e-9));

        // 3:1 rule level diff ≈ 9.542 dB
        assert!((distance_ratio_level_diff_db(3.0, 1.0) - 9.542425).abs() < 1e-4);
        assert!((three_to_one_min_distance_m(2.0 * f64::from(FEET_TO_METRES)) - 6.0 * f64::from(FEET_TO_METRES)).abs() < 1e-6);
        assert_eq!(xy_clamped_angle_deg(60.0), 90.0);
        assert_eq!(xy_clamped_angle_deg(150.0), 135.0);
    }

    #[test]
    fn changing_one_wall_updates_rt60_and_reflected_audio_while_preserving_direct_timing() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();
        let base = StudioRig::default(); // Small 10x10x10 ft, 2" wedge foam walls
        let mut wall_steel = base.clone();
        let steel_idx = surfaces
            .iter()
            .position(|s| s.name.to_ascii_lowercase().contains("sheet steel"))
            .expect("sheet steel surface must exist");
        wall_steel.surface_faces[0] = steel_idx;

        let a_base = analyze_studio_rig(&base, &sources, &mics, &surfaces);
        let a_steel = analyze_studio_rig(&wall_steel, &sources, &mics, &surfaces);

        assert!(
            a_steel.sabine_rt60_overall_s.unwrap() > a_base.sabine_rt60_overall_s.unwrap(),
            "swapping foam wall for steel must raise estimated RT60"
        );
        assert_eq!(a_base.direct_path.distance_m, a_steel.direct_path.distance_m);
        assert_eq!(
            a_base.direct_path.direct_delay_ms,
            a_steel.direct_path.direct_delay_ms
        );

        let mut impulse = vec![0.0_f32; 4800];
        impulse[40] = 0.8;
        let y_base = render_studio_rig(&impulse, 48000, &base, &sources, &mics, &surfaces);
        let y_steel = render_studio_rig(&impulse, 48000, &wall_steel, &sources, &mics, &surfaces);
        assert_ne!(y_base, y_steel);
    }

    #[test]
    fn changing_room_dimensions_updates_modes_and_rt60_and_aim_changes_polar_without_moving_mic() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();
        let base = StudioRig::default();
        let mut bigger = base.clone();
        bigger.set_dimension_m(1, 7.0);

        let a_base = analyze_studio_rig(&base, &sources, &mics, &surfaces);
        let a_big = analyze_studio_rig(&bigger, &sources, &mics, &surfaces);

        assert!(a_big.axial_modes[0].freq_hz < a_base.axial_modes[0].freq_hz);
        assert!(a_big.sabine_rt60_overall_s.unwrap() > a_base.sabine_rt60_overall_s.unwrap());

        let mut aimed = base.clone();
        aimed.active_mic_mut().aim.follow_source = false;
        aimed.active_mic_mut().aim.heading_z_deg = 90.0;
        let a_aimed = analyze_studio_rig(&aimed, &sources, &mics, &surfaces);

        assert_eq!(aimed.active_mic().offset_m, base.active_mic().offset_m);
        assert_eq!(a_aimed.direct_path.distance_m, a_base.direct_path.distance_m);
        assert!((a_aimed.direct_path.off_axis_deg - 90.0).abs() < 0.5);
        assert!(a_aimed.direct_path.polar_gain_linear < a_base.direct_path.polar_gain_linear - 0.3);

        let mut display_only = base.clone();
        display_only.unit = Unit::Millimetres;
        display_only.show_patterns = true;
        let a_disp = analyze_studio_rig(&display_only, &sources, &mics, &surfaces);
        assert_eq!(a_base, a_disp);
    }

    #[test]
    fn two_mic_three_to_one_rule_updates_with_position_and_capture_follows_actual_geometry() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();

        // Medium room (20x20x10 ft) so 6 ft fits comfortably inside the room from center source.
        let mut rig = StudioRig::default();
        rig.apply_preset(RoomPreset::Medium);
        rig.mics[0].offset_m = [0.0, 2.0 * FEET_TO_METRES, 0.0]; // Mic 1 at 2 ft
        rig.add_mic_slot();
        // Place Mic 2 at 3 ft (inside 6 ft 3:1 threshold -> shortfall!)
        rig.mics[1].offset_m = [0.0, 3.0 * FEET_TO_METRES, 0.0];
        rig.selected_mic = 1;
        rig.clamp_all();

        let a_short = analyze_studio_rig(&rig, &sources, &mics, &surfaces);
        assert!(a_short.placement_guidance.multi_mic_active);
        assert!((a_short.placement_guidance.three_to_one_min_distance_m - 6.0 * f64::from(FEET_TO_METRES)).abs() < 1e-4);
        assert_eq!(a_short.placement_guidance.slots[0].meets_three_to_one, None);
        assert_eq!(a_short.placement_guidance.slots[1].meets_three_to_one, Some(false));
        assert!((a_short.placement_guidance.slots[1].shortfall_m - 3.0 * f64::from(FEET_TO_METRES)).abs() < 1e-3);

        let mut impulse = vec![0.0_f32; 4800];
        impulse[40] = 0.8;
        let y_short = render_studio_rig(&impulse, 48000, &rig, &sources, &mics, &surfaces);

        // Move Mic 2 to 6 ft -> clears 3:1 shortfall & updates rendered capture!
        rig.mics[1].offset_m = [0.0, 6.0 * FEET_TO_METRES, 0.0];
        rig.clamp_all();
        let a_clear = analyze_studio_rig(&rig, &sources, &mics, &surfaces);
        assert_eq!(a_clear.placement_guidance.slots[1].meets_three_to_one, Some(true));
        assert!(a_clear.placement_guidance.slots[1].shortfall_m < 1e-6);
        assert!((a_clear.placement_guidance.slots[1].level_diff_vs_nearest_db - 9.5424).abs() < 0.02);

        let y_clear = render_studio_rig(&impulse, 48000, &rig, &sources, &mics, &surfaces);
        assert_ne!(y_short, y_clear, "moving Mic 2 to 6 ft must change rendered capture");

        // ORTF preset: 17 cm spacing, ±55° splayed cardioid capsules
        let mut ortf = rig.clone();
        ortf.mics[0].offset_m = [-0.085, 2.0 * FEET_TO_METRES, 0.0];
        ortf.mics[0].aim.follow_source = false;
        ortf.mics[0].aim.heading_z_deg = -55.0;
        ortf.mics[1].offset_m = [0.085, 2.0 * FEET_TO_METRES, 0.0];
        ortf.mics[1].aim.follow_source = false;
        ortf.mics[1].aim.heading_z_deg = 55.0;
        ortf.clamp_all();
        let a_ortf = analyze_studio_rig(&ortf, &sources, &mics, &surfaces);
        assert!((a_ortf.placement_guidance.slots[1].distance_from_nearest_mic_m - 0.17).abs() < 1e-4);
        let y_ortf = render_studio_rig(&impulse, 48000, &ortf, &sources, &mics, &surfaces);
        assert_ne!(y_ortf, y_clear);
    }
}
