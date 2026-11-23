use spectracraft_mono::{qa, studio_catalog, Result};
use std::{io::Write, path::Path};

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn parse_pattern(s: &str) -> Option<studio_catalog::Pattern> {
    match s.to_ascii_lowercase().as_str() {
        "omni" => Some(studio_catalog::Pattern::Omni),
        "wide_cardioid" | "wide" => Some(studio_catalog::Pattern::WideCardioid),
        "cardioid" => Some(studio_catalog::Pattern::Cardioid),
        "supercardioid" | "super" => Some(studio_catalog::Pattern::Supercardioid),
        "hypercardioid" | "hyper" => Some(studio_catalog::Pattern::Hypercardioid),
        "figure8" | "figure_8" | "figure-8" => Some(studio_catalog::Pattern::Figure8),
        _ => None,
    }
}

fn pattern_key(p: studio_catalog::Pattern) -> &'static str {
    match p {
        studio_catalog::Pattern::Omni => "omni",
        studio_catalog::Pattern::WideCardioid => "wide_cardioid",
        studio_catalog::Pattern::Cardioid => "cardioid",
        studio_catalog::Pattern::Supercardioid => "supercardioid",
        studio_catalog::Pattern::Hypercardioid => "hypercardioid",
        studio_catalog::Pattern::Figure8 => "figure8",
    }
}

fn room_preset_key(p: studio_catalog::RoomPreset) -> &'static str {
    match p {
        studio_catalog::RoomPreset::Small => "small",
        studio_catalog::RoomPreset::Medium => "medium",
        studio_catalog::RoomPreset::Large => "large",
        studio_catalog::RoomPreset::Open => "open",
        studio_catalog::RoomPreset::Direct => "direct",
    }
}

fn parse_room_preset(s: &str) -> studio_catalog::RoomPreset {
    match s.to_ascii_lowercase().as_str() {
        "medium" => studio_catalog::RoomPreset::Medium,
        "large" => studio_catalog::RoomPreset::Large,
        "open" | "open_field" => studio_catalog::RoomPreset::Open,
        "direct" => studio_catalog::RoomPreset::Direct,
        _ => studio_catalog::RoomPreset::Small,
    }
}

fn source_symbol_key(sym: studio_catalog::SourceSymbol) -> &'static str {
    match sym {
        studio_catalog::SourceSymbol::Unknown => "unknown",
        studio_catalog::SourceSymbol::Guitar => "guitar",
        studio_catalog::SourceSymbol::Amp => "amp",
        studio_catalog::SourceSymbol::Bowed => "bowed",
        studio_catalog::SourceSymbol::Wind => "wind",
        studio_catalog::SourceSymbol::Brass => "brass",
        studio_catalog::SourceSymbol::Percussion => "percussion",
        studio_catalog::SourceSymbol::Keyboard => "keyboard",
        studio_catalog::SourceSymbol::Voice => "voice",
        studio_catalog::SourceSymbol::Ensemble => "ensemble",
    }
}

fn studio_catalogs() -> Result<()> {
    let source_list = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let source_count = source_list.len();
    let sources: Vec<serde_json::Value> = source_list
        .into_iter()
        .enumerate()
        .map(|(index, s)| {
            serde_json::json!({
                "index": index,
                "id": s.id,
                "name": s.name,
                "category": s.category.label(),
                "family": s.family,
                "symbol": source_symbol_key(s.symbol),
                "published_range_hz": s.published_range_hz,
                "published_fundamental_notes": s.published_fundamental_notes,
                "published_practical_range_hz": s.published_practical_range_hz,
                "spectral_regions": {
                    "body_warmth_hz": s.spectral_regions.body_warmth_hz,
                    "presence_attack_hz": s.spectral_regions.presence_attack_hz,
                    "air_high_hz": s.spectral_regions.air_high_hz,
                    "formants_and_resonances_note": s.spectral_regions.formants_and_resonances_note,
                },
                "published_notes": s.published_notes,
                "source_references": s.source_references,
                "uncertainties": s.uncertainties,
                "radiation_label": s.radiation.label(),
                "radiation_alpha": s.radiation.directivity_alpha(),
                "synthesis_model_hint": s.synthesis_model_hint,
                "regen_model": {
                    "model_id": s.regen_model.model_id,
                    "model_label": s.regen_model.model_label,
                    "description": s.regen_model.description,
                    "physics_constraint": {
                        "attack_ms": s.regen_model.family_kind.physics_constraint().attack_ms,
                        "release_ms": s.regen_model.family_kind.physics_constraint().release_ms,
                        "breath_texture_amount": s.regen_model.family_kind.physics_constraint().breath_texture_amount,
                        "dynamic_brightness_amount": s.regen_model.family_kind.physics_constraint().dynamic_brightness_amount,
                        "harmonic_ceiling_hz": s.regen_model.family_kind.physics_constraint().harmonic_ceiling_hz,
                        "resonance_contour_summary": s.regen_model.family_kind.physics_constraint().resonance_contour_summary,
                        "texture_mechanism": s.regen_model.family_kind.physics_constraint().texture_mechanism,
                        "plausibility_note": s.regen_model.family_kind.physics_constraint().plausibility_note,
                    },
                },
            })
        })
        .collect();

    let mics: Vec<serde_json::Value> = studio_catalog::mic_catalog()
        .into_iter()
        .enumerate()
        .map(|(index, m)| {
            let shared_explanation = m.shared_model_explanation();
            let offaxis_json = m.published_offaxis.as_ref().map(|o| {
                serde_json::json!({
                    "freqs_hz": o.freqs_hz,
                    "atten_90deg_db": o.atten_90deg_db,
                    "atten_180deg_db": o.atten_180deg_db,
                })
            });
            serde_json::json!({
                "index": index,
                "id": m.id,
                "brand": m.brand,
                "model": m.model,
                "source_url": m.source_url,
                "transducer": m.transducer.label(),
                "default_pattern": pattern_key(m.pattern),
                "default_pattern_label": m.pattern.label(),
                "published_type": m.published_type,
                "published_patterns": m.published_patterns,
                "published_curves": m.published_curves,
                "published_primary_curve": m.published_primary_curve,
                "published_fr_points": m.published_fr_points,
                "published_offaxis": offaxis_json,
                "published_range_text": m.published_range_text,
                "published_range_hz": m.published_range_hz,
                "published_sensitivity_dbv": m.published_sensitivity_dbv,
                "published_self_noise_dba": m.published_self_noise_dba,
                "published_max_spl_db": m.published_max_spl_db,
                "published_impedance_ohm": m.published_impedance_ohm,
                "published_released_year": m.published_released_year,
                "published_measurement_dist_cm": m.published_measurement_dist_cm,
                "published_power": m.published_power,
                "source_document": m.source_document,
                "uncertainties": m.uncertainties,
                "renderer_profile_id": m.renderer_profile.id(),
                "renderer_profile_description": m.renderer_profile.description(),
                "curve_provenance": m.curve_provenance,
                "published_notes": m.published_notes,
                "shared_model_explanation": shared_explanation,
            })
        })
        .collect();

    let surfaces: Vec<serde_json::Value> = studio_catalog::surface_catalog()
        .into_iter()
        .enumerate()
        .map(|(index, s)| {
            serde_json::json!({
                "index": index,
                "id": s.id,
                "name": s.name,
                "category": s.category,
                "material_type": s.material_type,
                "thickness_mm": s.thickness_mm,
                "assembly": s.assembly,
                "incidence": s.incidence,
                "standard": s.standard,
                "response_kind": s.response_kind,
                "raw_absorption_6band": s.absorption_6band,
                "effective_absorption_6band": s.effective_absorption(),
                "pressure_reflectance_6band": s.pressure_reflectance(),
                "mean_reflectance": s.mean_reflectance(),
                "biot": {
                    "porosity": s.biot.porosity,
                    "airflow_resistivity_pa_s_m2": s.biot.airflow_resistivity_pa_s_m2,
                    "tortuosity": s.biot.tortuosity,
                    "vcl_um": s.biot.vcl_um,
                    "tcl_um": s.biot.tcl_um,
                    "standard_ref": s.biot.standard_ref,
                    "status": s.biot.status,
                },
                "provenance": s.provenance,
                "notes": s.notes,
                "uncertainties": s.uncertainties,
            })
        })
        .collect();

    let presets: Vec<serde_json::Value> = studio_catalog::RoomPreset::ALL
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "key": room_preset_key(p),
                "label": p.label(),
                "dimensions_m": p.dimensions_m(),
                "default_surface_faces": p.default_surface_faces(),
            })
        })
        .collect();

    let patterns: Vec<serde_json::Value> = studio_catalog::Pattern::ALL
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "key": pattern_key(p),
                "label": p.label(),
                "alpha": p.alpha(),
            })
        })
        .collect();

    print_json(&serde_json::json!({
        "bands_hz": studio_catalog::BANDS_HZ,
        "face_labels": studio_catalog::FACE_LABELS,
        "sources": sources,
        "mics": mics,
        "surfaces": surfaces,
        "room_presets": presets,
        "patterns": patterns,
        "loose_profiles": spectracraft_mono::reference_profiles::loose_profile_library(),
        "catalog_provenance": {
            "engine": "spectracraft-mono::studio_catalog (Rust + offline capture-reference)",
            "source_count": source_count,
            "mic_count": studio_catalog::mic_catalog().len(),
            "surface_count": studio_catalog::surface_catalog().len(),
            "mic_archive_note": "972 microphones across 88 brands recovered offline from Mic Archive (939 with digitized curves, 667 with published off-axis polar tables) + 32 architectural 6-band acoustic material profiles in capture-reference. Zero runtime network dependency; published specs/curves are reference only, never measured emulations."
        }
    }))
}

fn parse_studio_rig_config(
    cfg: &serde_json::Value,
    sources: &[studio_catalog::SourceEntry],
    mics: &[studio_catalog::MicEntry],
    surfaces: &[studio_catalog::SurfaceEntry],
) -> studio_catalog::StudioRig {
    let mut rig = studio_catalog::StudioRig::default();
    if let Some(p) = cfg.get("room_preset").and_then(|v| v.as_str()) {
        rig.apply_preset(parse_room_preset(p));
    }
    if let Some(dims) = cfg.get("dimensions_m").and_then(|v| v.as_array()) {
        if dims.len() == 3 {
            rig.dimensions_m = [
                dims[0].as_f64().unwrap_or(3.048) as f32,
                dims[1].as_f64().unwrap_or(3.048) as f32,
                dims[2].as_f64().unwrap_or(3.048) as f32,
            ];
        }
    }
    if let Some(idx) = cfg.get("source_index").and_then(|v| v.as_u64()) {
        rig.source_index = (idx as usize).min(sources.len().saturating_sub(1));
    }
    if let Some(off) = cfg.get("source_offset_m").and_then(|v| v.as_array()) {
        if off.len() == 3 {
            rig.source_offset_m = [
                off[0].as_f64().unwrap_or(0.0) as f32,
                off[1].as_f64().unwrap_or(0.0) as f32,
                off[2].as_f64().unwrap_or(0.0) as f32,
            ];
        }
    }
    if let Some(v) = cfg.get("source_lock_center_xy").and_then(|v| v.as_bool()) {
        rig.source_lock_center_xy = v;
    }
    if let Some(v) = cfg.get("source_lock_height_5ft").and_then(|v| v.as_bool()) {
        rig.source_lock_height_5ft = v;
    }
    if let Some(faces) = cfg.get("surface_faces").and_then(|v| v.as_array()) {
        if faces.len() == 6 {
            for (i, f) in faces.iter().enumerate() {
                if let Some(sidx) = f.as_u64() {
                    rig.surface_faces[i] = (sidx as usize).min(surfaces.len().saturating_sub(1));
                }
            }
        }
    }
    if let Some(t) = cfg.get("temperature_c").and_then(|v| v.as_f64()) {
        rig.temperature_c = t as f32;
    }
    if let Some(mic_arr) = cfg.get("mics").and_then(|v| v.as_array()) {
        if !mic_arr.is_empty() {
            let mut parsed = Vec::new();
            for (mi, mv) in mic_arr.iter().enumerate() {
                let mut slot = studio_catalog::MicSlot::sm7_baseline();
                slot.label = mv
                    .get("label")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Mic {}", mi + 1));
                if let Some(ci) = mv.get("catalog_index").and_then(|v| v.as_u64()) {
                    slot.catalog_index = (ci as usize).min(mics.len().saturating_sub(1));
                }
                if let Some(ps) = mv.get("pattern_override").and_then(|v| v.as_str()) {
                    slot.pattern_override = parse_pattern(ps);
                }
                if let Some(off) = mv.get("offset_m").and_then(|v| v.as_array()) {
                    if off.len() == 3 {
                        slot.offset_m = [
                            off[0].as_f64().unwrap_or(0.0) as f32,
                            off[1].as_f64().unwrap_or(0.6096) as f32,
                            off[2].as_f64().unwrap_or(0.0) as f32,
                        ];
                    }
                }
                if let Some(aim) = mv.get("aim") {
                    slot.aim.tilt_x_deg =
                        aim.get("tilt_x_deg").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                    slot.aim.roll_y_deg =
                        aim.get("roll_y_deg").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                    slot.aim.heading_z_deg = aim
                        .get("heading_z_deg")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    slot.aim.follow_source = aim
                        .get("follow_source")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                }
                if let Some(en) = mv.get("enabled").and_then(|v| v.as_bool()) {
                    slot.enabled = en;
                }
                parsed.push(slot);
            }
            rig.mics = parsed;
        }
    }
    if let Some(sel) = cfg.get("selected_mic").and_then(|v| v.as_u64()) {
        rig.selected_mic = (sel as usize).min(rig.mics.len().saturating_sub(1));
    }
    rig.clamp_all();
    rig
}

fn studio_analysis(config_path: &Path) -> Result<()> {
    let cfg: serde_json::Value = serde_json::from_reader(std::fs::File::open(config_path)?)?;
    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let mics = studio_catalog::mic_catalog();
    let surfaces = studio_catalog::surface_catalog();
    let rig = parse_studio_rig_config(&cfg, &sources, &mics, &surfaces);
    let analysis =
        spectracraft_mono::acoustic_math::analyze_studio_rig(&rig, &sources, &mics, &surfaces);
    print_json(&analysis)
}

fn studio_capture(input: &Path, config_path: &Path, out: &Path) -> Result<()> {
    let (spec, samples) = qa::read_audio(input)?;
    if spec.channels != 1 {
        return Err("Expected mono WAV for studio-capture".into());
    }
    let cfg: serde_json::Value = serde_json::from_reader(std::fs::File::open(config_path)?)?;
    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let mics = studio_catalog::mic_catalog();
    let surfaces = studio_catalog::surface_catalog();
    let rig = parse_studio_rig_config(&cfg, &sources, &mics, &surfaces);

    let max_order = cfg
        .get("max_order")
        .and_then(|v| v.as_i64())
        .unwrap_or(4)
        .clamp(0, 4) as i32;

    // Render all enabled mic slots (or active mic if none explicitly enabled) and sum cleanly
    let active_indices: Vec<usize> = rig
        .mics
        .iter()
        .enumerate()
        .filter(|(_, m)| m.enabled)
        .map(|(i, _)| i)
        .collect();
    let indices = if active_indices.is_empty() {
        vec![rig.selected_mic.min(rig.mics.len().saturating_sub(1))]
    } else {
        active_indices
    };

    let mut primary_plan = studio_catalog::capture_paths_6band(
        &rig,
        &sources,
        &mics,
        &surfaces,
        max_order,
        studio_catalog::Timing::AlignDirect,
    );
    let mut mixed = vec![0.0_f32; samples.len()];
    let inv_count = 1.0 / (indices.len() as f32);
    for (k, &mi) in indices.iter().enumerate() {
        let mut slot_rig = rig.clone();
        slot_rig.selected_mic = mi;
        let plan = studio_catalog::capture_paths_6band(
            &slot_rig,
            &sources,
            &mics,
            &surfaces,
            max_order,
            studio_catalog::Timing::AlignDirect,
        );
        if k == 0 || mi == rig.selected_mic {
            primary_plan = plan.clone();
        }
        let rendered = spectracraft_mono::capture::render_studio_rig(
            &samples,
            spec.sample_rate,
            &slot_rig,
            &sources,
            &mics,
            &surfaces,
        );
        for (dst, &v) in mixed.iter_mut().zip(rendered.iter()) {
            *dst += v * inv_count;
        }
    }

    // Write captured WAV as 32-bit float (lossless, preserves headroom without clipping)
    let out_file = std::fs::File::create(out)?;
    let out_spec = hound::WavSpec {
        channels: 1,
        sample_rate: spec.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::new(std::io::BufWriter::new(out_file), out_spec)?;
    let mut peak = 0.0_f64;
    let mut sq_sum = 0.0_f64;
    for &s in &mixed {
        let clamped = if s.is_finite() { s } else { 0.0 };
        peak = peak.max(f64::from(clamped).abs());
        sq_sum += f64::from(clamped) * f64::from(clamped);
        writer.write_sample(clamped)?;
    }
    writer.finalize()?;

    let sha256 = spectracraft_mono::digest::sha256(&std::fs::read(out)?);
    let rms = (sq_sum / mixed.len().max(1) as f64).sqrt();
    let peak_dbfs = if peak > 1e-12 {
        Some(20.0 * peak.log10())
    } else {
        None
    };
    let rms_dbfs = if rms > 1e-12 {
        Some(20.0 * rms.log10())
    } else {
        None
    };

    let active_mic = rig.active_mic();
    let eff_pat = active_mic.effective_pattern(&mics);
    let eff_faces: Vec<serde_json::Value> = (0..6)
        .map(|f| {
            let active = match rig.room_preset {
                studio_catalog::RoomPreset::Direct => false,
                studio_catalog::RoomPreset::Open => f == 4,
                _ => true,
            };
            let s = &surfaces[rig.surface_faces[f]];
            serde_json::json!({
                "face": studio_catalog::FACE_LABELS[f],
                "active": active,
                "surface_index": rig.surface_faces[f],
                "surface_name": s.name,
                "raw_absorption_6band": s.absorption_6band,
                "effective_absorption_6band": s.effective_absorption(),
                "pressure_reflectance_6band": s.pressure_reflectance(),
            })
        })
        .collect();

    let acoustic_analysis =
        spectracraft_mono::acoustic_math::analyze_studio_rig(&rig, &sources, &mics, &surfaces);

    print_json(&serde_json::json!({
        "sha256": sha256,
        "samples": mixed.len(),
        "sample_rate": spec.sample_rate,
        "peak_dbfs": peak_dbfs,
        "rms_dbfs": rms_dbfs,
        "speed_of_sound_mps": primary_plan.speed_of_sound_mps,
        "distance_m": primary_plan.distance_m,
        "direct_delay_ms": primary_plan.direct_arrival_s * 1000.0,
        "direct_pickup_gain": primary_plan.direct_pickup_gain,
        "first_reflection_ms": primary_plan.first_reflection_s.map(|s| s * 1000.0),
        "last_path_ms": primary_plan.last_path_s * 1000.0,
        "path_count": primary_plan.paths.len(),
        "six_band_drr_db": primary_plan.six_band_drr_db,
        "source_pos_m": rig.source_pos_m(),
        "active_mic_pattern": pattern_key(eff_pat),
        "active_mic_pattern_label": eff_pat.label(),
        "effective_faces": eff_faces,
        "room_preset": room_preset_key(rig.room_preset),
        "acoustic_analysis": acoustic_analysis,
    }))
}

fn render_dry_source(
    cues_path: &Path,
    source_index: usize,
    out: &Path,
    voicing_path: Option<&Path>,
) -> Result<()> {
    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let idx = source_index.min(sources.len().saturating_sub(1));
    let source = &sources[idx];
    let file = std::fs::File::open(cues_path)?;
    if file.metadata()?.len() > 16 * 1024 * 1024 {
        return Err("Cue file is too large".into());
    }
    let performance: spectracraft_mono::reperform::Performance =
        serde_json::from_reader(std::io::BufReader::new(file))?;
    let voicing: spectracraft_mono::reperform::VoicingControls = match voicing_path {
        Some(p) => serde_json::from_reader(std::fs::File::open(p)?)?,
        None => spectracraft_mono::reperform::VoicingControls::default(),
    };
    let samples = performance.synthesize_dry_for_source_with_voicing(source, &voicing)?;
    let summary = spectracraft_mono::reperform::measure_spectral_summary(&samples, performance.rate());
    spectracraft_mono::pipeline::write_wav_f32_atomic(out, performance.rate(), &samples)?;
    let sha256 = spectracraft_mono::digest::sha256(&std::fs::read(out)?);
    print_json(&serde_json::json!({
        "sha256": sha256,
        "source_index": idx,
        "source_id": source.id,
        "source_name": source.name,
        "synthesis_model_hint": source.synthesis_model_hint,
        "regen_model_id": source.regen_model.model_id,
        "regen_model_label": source.regen_model.model_label,
        "voicing": voicing,
        "original_audio_opened": false,
        "samples": samples.len(),
        "sample_rate": performance.rate(),
        "note_count": performance.notes().len(),
        "spectral": summary,
    }))
}

/// Stage 2 — Surgical Region Editing on the cached dry take.
fn dry_region_edit(
    cues_path: &Path,
    cached_dry_path: &Path,
    cfg_path: &Path,
    out_edited: &Path,
    out_before: &Path,
    out_after: &Path,
) -> Result<()> {
    let (spec, cached_dry) = qa::read_audio(cached_dry_path)?;
    if spec.channels != 1 {
        return Err("Expected mono cached dry WAV for dry-region-edit".into());
    }
    let performance: spectracraft_mono::reperform::Performance =
        serde_json::from_reader(std::fs::File::open(cues_path)?)?;
    let cfg: serde_json::Value = serde_json::from_reader(std::fs::File::open(cfg_path)?)?;
    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let source_idx = cfg
        .get("source_index")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as usize;
    let source = &sources[source_idx.min(sources.len().saturating_sub(1))];
    let voicing: spectracraft_mono::reperform::VoicingControls = cfg
        .get("voicing")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let edit_val = cfg.get("edit").cloned().unwrap_or(cfg.clone());
    let edit: spectracraft_mono::region_edit::DryRegionEdit = serde_json::from_value(edit_val)?;

    let res = spectracraft_mono::region_edit::apply_dry_region_edit(
        &cached_dry,
        &performance,
        source,
        &voicing,
        &edit,
    )?;

    spectracraft_mono::pipeline::write_wav_f32_atomic(out_edited, spec.sample_rate, &res.edited_dry)?;
    spectracraft_mono::pipeline::write_wav_f32_atomic(out_before, spec.sample_rate, &res.before_slice)?;
    spectracraft_mono::pipeline::write_wav_f32_atomic(out_after, spec.sample_rate, &res.after_slice)?;

    let edited_sha256 = spectracraft_mono::digest::sha256(&std::fs::read(out_edited)?);
    let before_sha256 = spectracraft_mono::digest::sha256(&std::fs::read(out_before)?);
    let after_sha256 = spectracraft_mono::digest::sha256(&std::fs::read(out_after)?);
    let spectral =
        spectracraft_mono::reperform::measure_spectral_summary(&res.edited_dry, spec.sample_rate);

    print_json(&serde_json::json!({
        "edited_sha256": edited_sha256,
        "before_slice_sha256": before_sha256,
        "after_slice_sha256": after_sha256,
        "start_sample": res.start_sample,
        "end_sample": res.end_sample,
        "crossfade_samples": res.crossfade_samples,
        "affected_notes": res.affected_notes,
        "effective_action": res.effective_action,
        "unresolved_note": res.unresolved_note,
        "outside_bit_identical": res.outside_bit_identical,
        "original_audio_opened": false,
        "sample_rate": spec.sample_rate,
        "samples": res.edited_dry.len(),
        "spectral": spectral,
    }))
}

/// Stages 3, 4, 5 — Independently bypassable post-dry pipeline (Mic, Space/Room, Plugin Slot).
fn pipeline_render(cached_dry_path: &Path, cfg_path: &Path, out: &Path) -> Result<()> {
    let (spec, cached_dry) = qa::read_audio(cached_dry_path)?;
    if spec.channels != 1 {
        return Err("Expected mono cached dry WAV for pipeline-render".into());
    }
    let cfg: serde_json::Value = serde_json::from_reader(std::fs::File::open(cfg_path)?)?;
    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let mics = studio_catalog::mic_catalog();
    let surfaces = studio_catalog::surface_catalog();
    let rig_val = cfg.get("rig").unwrap_or(&cfg);
    let rig = parse_studio_rig_config(rig_val, &sources, &mics, &surfaces);
    let mic_enabled = cfg
        .get("mic_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let room_enabled = cfg
        .get("room_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let plugin_slot: spectracraft_mono::pipeline::PluginSlotConfig = cfg
        .get("plugin_slot")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let pipe_cfg = spectracraft_mono::pipeline::PostDryPipelineConfig {
        mic_enabled,
        room_enabled,
        rig: rig.clone(),
        plugin_slot,
    };

    let out_res = spectracraft_mono::pipeline::run_post_dry_pipeline(
        &cached_dry,
        spec.sample_rate,
        &pipe_cfg,
        &sources,
        &mics,
        &surfaces,
    );
    spectracraft_mono::pipeline::write_wav_f32_atomic(out, spec.sample_rate, &out_res.samples)?;
    let wav_sha256 = spectracraft_mono::digest::sha256(&std::fs::read(out)?);
    let spectral =
        spectracraft_mono::reperform::measure_spectral_summary(&out_res.samples, spec.sample_rate);
    let acoustic_analysis =
        spectracraft_mono::acoustic_math::analyze_studio_rig(&rig, &sources, &mics, &surfaces);

    print_json(&serde_json::json!({
        "wav_sha256": wav_sha256,
        "samples": out_res.samples.len(),
        "sample_rate": spec.sample_rate,
        "summary": out_res.summary,
        "spectral": spectral,
        "acoustic_analysis": acoustic_analysis,
    }))
}

/// Complete shape-guided synthesis from extracted cues + Capture Lab prompt ONLY.
/// Original WAV is never opened or blended: cues -> source-voiced dry -> optional stages 3/4/5.
fn studio_prompt_render(cues_path: &Path, config_path: &Path, out: &Path) -> Result<()> {
    let cues_bytes = std::fs::read(cues_path)?;
    let shape_sha256 = spectracraft_mono::digest::sha256(&cues_bytes);
    let performance: spectracraft_mono::reperform::Performance =
        serde_json::from_slice(&cues_bytes)?;
    let cfg: serde_json::Value = serde_json::from_reader(std::fs::File::open(config_path)?)?;
    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let mics = studio_catalog::mic_catalog();
    let surfaces = studio_catalog::surface_catalog();
    let rig = parse_studio_rig_config(&cfg, &sources, &mics, &surfaces);
    let voicing: spectracraft_mono::reperform::VoicingControls = cfg
        .get("voicing")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let mic_enabled = cfg
        .get("mic_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let room_enabled = cfg
        .get("room_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let plugin_slot: spectracraft_mono::pipeline::PluginSlotConfig = cfg
        .get("plugin_slot")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let source = &sources[rig.source_index.min(sources.len().saturating_sub(1))];
    let dry_samples = performance.synthesize_dry_for_source_with_voicing(source, &voicing)?;

    let pipe_cfg = spectracraft_mono::pipeline::PostDryPipelineConfig {
        mic_enabled,
        room_enabled,
        rig: rig.clone(),
        plugin_slot,
    };
    let pipe_out = spectracraft_mono::pipeline::run_post_dry_pipeline(
        &dry_samples,
        performance.rate(),
        &pipe_cfg,
        &sources,
        &mics,
        &surfaces,
    );

    spectracraft_mono::pipeline::write_wav_f32_atomic(out, performance.rate(), &pipe_out.samples)?;

    let captured_sha256 = spectracraft_mono::digest::sha256(&std::fs::read(out)?);
    let acoustic_analysis =
        spectracraft_mono::acoustic_math::analyze_studio_rig(&rig, &sources, &mics, &surfaces);
    let spectral =
        spectracraft_mono::reperform::measure_spectral_summary(&pipe_out.samples, performance.rate());

    print_json(&serde_json::json!({
        "shape_sha256": shape_sha256,
        "dry_sha256": pipe_out.summary.dry_sha256,
        "captured_sha256": captured_sha256,
        "original_audio_opened": false,
        "source_index": rig.source_index,
        "source_name": source.name,
        "voicing": voicing,
        "pipeline_summary": pipe_out.summary,
        "samples": pipe_out.samples.len(),
        "sample_rate": performance.rate(),
        "note_count": performance.notes().len(),
        "spectral": spectral,
        "acoustic_analysis": acoustic_analysis,
    }))
}

fn reference_shape(input_wav: &Path, cues_path: &Path, source_idx: Option<usize>) -> Result<()> {
    let (spec, samples) = qa::read_audio(input_wav)?;
    if spec.channels != 1 {
        return Err("Expected mono WAV for reference-shape".into());
    }
    let performance: spectracraft_mono::reperform::Performance =
        serde_json::from_reader(std::fs::File::open(cues_path)?)?;
    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let selected = source_idx.and_then(|i| sources.get(i));
    let shape = spectracraft_mono::shape_profile::extract_reference_shape(
        &samples,
        spec.sample_rate,
        &performance,
        selected,
    )?;
    print_json(&shape)
}

fn fixture(out: &Path) -> Result<()> {
    let (samples, mut meta) = qa::six_note();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::new(std::io::BufWriter::new(file), spec)?;
    for sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    meta["sha256"] = spectracraft_mono::digest::sha256(&std::fs::read(out)?).into();
    print_json(&meta)
}

fn gap_grid(input: &Path) -> Result<()> {
    let (spec, samples) = qa::read_audio(input)?;
    let channels = spec.channels as usize;
    let frames = samples.len() / channels;
    let mut rows = Vec::new();
    for fraction in [0.17, 0.29, 0.41, 0.53, 0.67, 0.79] {
        for ms in [0.5, 2., 4.] {
            let start = (frames as f64 * fraction) as usize;
            let end = (start + (spec.sample_rate as f64 * ms / 1000.).round() as usize).min(frames);
            let mut damaged = samples.clone();
            damaged[start * channels..end * channels].fill(0.);
            let (rendered, status) = spectracraft_mono::gap::fill(
                &damaged,
                spec.sample_rate,
                channels,
                start,
                end,
                true,
            );
            if rendered[..start * channels] != damaged[..start * channels]
                || rendered[end * channels..] != damaged[end * channels..]
            {
                return Err("Out-of-gap mutation".into());
            }
            if status != "filled" && rendered != damaged {
                return Err("Declined patch changed".into());
            }
            let reference = &samples[start * channels..end * channels];
            let before = reference.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
            let after = rendered[start * channels..end * channels]
                .iter()
                .zip(reference)
                .map(|(a, b)| (*a as f64 - *b as f64).powi(2))
                .sum::<f64>();
            rows.push(serde_json::json!({
                "start_frame": start, "ms": ms, "decision": status,
                "error_reduction_db": (before > 0.).then(|| 10. * (before / after.max(1e-30)).log10())
            }));
        }
    }
    print_json(&rows)
}

fn preflight(root: &Path) -> Result<()> {
    let commands = [
        vec![
            "metadata",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--no-deps",
        ],
        vec![
            "tree",
            "--locked",
            "--offline",
            "--no-default-features",
            "-e",
            "normal,build",
        ],
        vec![
            "check",
            "--locked",
            "--offline",
            "--lib",
            "--no-default-features",
        ],
    ];
    let mut checks = Vec::new();
    for args in commands {
        let result = std::process::Command::new("cargo")
            .args(&args)
            .current_dir(root)
            .output();
        checks.push(match result {
            Ok(process) => serde_json::json!({
                "command": args, "passed": process.status.success(),
                "stdout": String::from_utf8_lossy(&process.stdout),
                "stderr": String::from_utf8_lossy(&process.stderr)
            }),
            Err(error) => {
                serde_json::json!({"command": args, "passed": false, "error": error.to_string()})
            }
        });
    }
    let passed = checks.iter().all(|value| value["passed"] == true);
    print_json(&serde_json::json!({
        "checks": checks, "native_device_smoke": "not run", "windows": "not verified",
        "advisories": "not checked", "audio_quality": "not assessed"
    }))?;
    std::io::stdout().flush()?;
    if !passed {
        return Err("Preflight failed".into());
    }
    Ok(())
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let args: Vec<_> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["guitar-boundaries",root] => {let result=qa::guitar_boundaries(Path::new(root))?;print_json(&result)?;if result["passed"]!=true{return Err("Archived boundary parity failed".into());}Ok(())}
        ["guitar-legacy",root] => print_json(&qa::guitar_legacy(Path::new(root))?),
        ["guitar-verify",input,out] => print_json(&qa::guitar_verify(Path::new(input),Path::new(out))?),
        ["guitar-variants",input,cues,out,selection] => print_json(&qa::export_guitar_variants(Path::new(input),Path::new(cues),Path::new(out),selection,spectracraft_mono::capture::DryReviewStatus::NotReviewed)?),
        ["guitar-variants",input,cues,out,selection,"--human-approved"] => print_json(&qa::export_guitar_variants(Path::new(input),Path::new(cues),Path::new(out),selection,spectracraft_mono::capture::DryReviewStatus::ApprovedByHuman)?),
        ["guitar-pitch-check",reference,candidate,start_s,end_s] => print_json(&qa::guitar_pitch_check(Path::new(reference),Path::new(candidate),start_s.parse()?,end_s.parse()?)?),
        ["cue-passes",input,out] => print_json(&qa::cue_passes(Path::new(input),Path::new(out))?),
        ["verify-corpus",prior,independent,six] => {
            let load=|path:&str|->Result<serde_json::Value>{Ok(serde_json::from_reader(std::fs::File::open(path)?)?)};
            let result=spectracraft_mono::metrics::verify_corpus_reports(&load(prior)?,&load(independent)?,&load(six)?)?;
            print_json(&result)?;if result["passed"]!=true{return Err("Frozen corpus acceptance failed".into());}Ok(())
        }
        ["verify-flute",input] => {
            let report=serde_json::from_reader(std::fs::File::open(input)?)?;
            let result=spectracraft_mono::metrics::verify_flute_report(&report)?;
            print_json(&result)?;if result["passed"]!=true{return Err("Frozen flute acceptance failed".into());}Ok(())
        }
        ["flute-layers",input] => print_json(&qa::flute_layers(Path::new(input))?),
        ["faint-export",input,out] => print_json(&qa::faint_export(Path::new(input),Path::new(out))?),
        ["cutoff-export",input,out] => print_json(&qa::cutoff_export(Path::new(input),Path::new(out))?),
        ["flute-faint",input] => {let r=qa::flute_faint(Path::new(input))?;print_json(&r)?;if r["passed"]!=true{return Err("Frozen faint-ridge acceptance failed".into());}Ok(())},
        ["flute-cutoffs",input] => print_json(&qa::flute_cutoffs(Path::new(input))?),
        ["flute-trace",input] => print_json(&qa::flute_trace(Path::new(input))?),
        ["gesture",input,out] => print_json(&qa::gesture(Path::new(input),Path::new(out))?),
        ["accent-figure",input,out] => print_json(&spectracraft_mono::figures::accent(Path::new(input),Path::new(out))?),
        #[cfg(feature="bench")]
        ["reference-match",input,out] => print_json(&spectracraft_mono::room_bench::reference_match(Path::new(input),Path::new(out))?),
        #[cfg(feature="bench")]
        ["response-figure",out] => spectracraft_mono::room_bench::response_figure(Path::new(out)),
        #[cfg(feature="bench")]
        ["reference-figures",input,out] => spectracraft_mono::room_bench::reference_figures(Path::new(input),Path::new(out)),
        #[cfg(feature="bench")]
        ["capture-figures",root] => print_json(&spectracraft_mono::room_bench::capture_figures(Path::new(root))?),
        #[cfg(feature="bench")]
        ["capture-experiments",input,out] => print_json(&spectracraft_mono::room_bench::capture_experiments(Path::new(input),Path::new(out))?),
        #[cfg(feature="bench")]
        ["masked-holdout",root] => print_json(&spectracraft_mono::bench::masked_holdout(Path::new(root))?),
        #[cfg(feature="bench")]
        ["smoke-export",root,out] => print_json(&spectracraft_mono::bench::smoke_export(Path::new(root),Path::new(out))?),
        #[cfg(feature="bench")]
        ["smoke",root] => print_json(&spectracraft_mono::bench::smoke(Path::new(root))?),
        #[cfg(feature="bench")]
        ["round2",root] => print_json(&spectracraft_mono::bench::round2(Path::new(root))?),
        #[cfg(feature="bench")]
        ["sampler-run",root,out,mode] => print_json(&spectracraft_mono::bench::sampler_run(Path::new(root),Path::new(out),mode)?),
        #[cfg(feature="bench")]
        ["round3",root] => print_json(&spectracraft_mono::bench::round3(Path::new(root))?),
        #[cfg(feature="bench")]
        ["six-samples",root] => print_json(&spectracraft_mono::bench::six_samples(Path::new(root))?),
        #[cfg(feature="bench")]
        ["independent",root] => print_json(&spectracraft_mono::bench::independent(Path::new(root))?),
        ["gap-corpus", input] => print_json(&qa::gap_corpus(Path::new(input))?),
        ["gap-grid", input] => gap_grid(Path::new(input)),
        ["engineering", input] => print_json(&qa::engineering_checks(Path::new(input))?),
        ["preflight", root] => preflight(Path::new(root)),
        ["preflight", root, platform, desktop, advisories] => {let r=spectracraft_mono::preflight::check(Path::new(root),platform,*desktop=="desktop",*advisories=="advisories")?;print_json(&r)?;if r["passed"]!=true{return Err("Requested preflight gates did not pass".into());}Ok(())},
        ["six-note", out] => fixture(Path::new(out)),
        ["audit-migration", retired, active] => {
            print_json(&spectracraft_mono::migration::audit(Path::new(retired), Path::new(active))?)
        }
        ["compare-pcm", left, right] => {
            let (a, x) = qa::read_audio(Path::new(left))?;
            let (b, y) = qa::read_audio(Path::new(right))?;
            if a != b || x != y { return Err("WAV spec or decoded PCM differs".into()); }
            println!("Identical WAV spec and decoded PCM: {} samples", x.len());
            Ok(())
        }
        ["source-inventory",root] => print_json(&spectracraft_mono::migration::source_inventory(Path::new(root))?),
        ["verify-source",root,manifest] => {let expected:serde_json::Value=serde_json::from_reader(std::fs::File::open(manifest)?)?;let actual=spectracraft_mono::migration::source_inventory(Path::new(root))?;if actual!=expected{return Err("Source bundle fingerprints differ".into());}println!("All source fingerprints match");Ok(())},
        ["hash", input] => {
            let hash = spectracraft_mono::digest::sha256(&std::fs::read(input)?);
            println!("{hash}  {input}");
            Ok(())
        }
        ["stereo", input] => {
            let (spec, samples) = qa::read_audio(Path::new(input))?;
            if spec.channels != 2 { return Err("Expected stereo WAV".into()); }
            print_json(&qa::stereo(&samples)?)
        }
        ["studio-catalogs"] => studio_catalogs(),
        ["studio-analysis", cfg] => studio_analysis(Path::new(cfg)),
        ["studio-capture", input, cfg, out] => {
            studio_capture(Path::new(input), Path::new(cfg), Path::new(out))
        }
        ["studio-prompt-render", cues, cfg, out] => {
            studio_prompt_render(Path::new(cues), Path::new(cfg), Path::new(out))
        }
        ["render-dry-source", cues, source_idx, out] => {
            render_dry_source(Path::new(cues), source_idx.parse()?, Path::new(out), None)
        }
        ["render-dry-source", cues, source_idx, out, voicing_cfg] => render_dry_source(
            Path::new(cues),
            source_idx.parse()?,
            Path::new(out),
            Some(Path::new(voicing_cfg)),
        ),
        ["dry-region-edit", cues, dry, cfg, out_edited, out_before, out_after] => dry_region_edit(
            Path::new(cues),
            Path::new(dry),
            Path::new(cfg),
            Path::new(out_edited),
            Path::new(out_before),
            Path::new(out_after),
        ),
        ["pipeline-render", dry, cfg, out] => {
            pipeline_render(Path::new(dry), Path::new(cfg), Path::new(out))
        }
        ["reference-shape", input, cues] => {
            reference_shape(Path::new(input), Path::new(cues), None)
        }
        ["reference-shape", input, cues, source_idx] => {
            reference_shape(Path::new(input), Path::new(cues), Some(source_idx.parse()?))
        }
        ["spectrogram", input, out] => {
            let (spec, samples) = qa::read_audio(Path::new(input))?;
            if spec.channels != 1 {
                return Err("Expected mono WAV; channel folding must be explicit".into());
            }
            qa::write_new(Path::new(out), qa::spectrogram_svg(&samples, spec.sample_rate)?.as_bytes())
        }
        _ => Err(concat!(
            "Usage: spectracraft-qa stereo INPUT | engineering INPUT | spectrogram INPUT OUTPUT.svg | ",
            "six-note OUTPUT.wav | gap-grid INPUT | gap-corpus INPUT_DIR | compare-pcm LEFT RIGHT | ",
            "hash FILE | audit-migration RETIRED_DIR CRATE | source-inventory CRATE | verify-source CRATE MANIFEST | ",
            "preflight CRATE [PLATFORM core|desktop skip|advisories] | guitar-legacy|guitar-boundaries GUITAR_ROOT | ",
            "guitar-verify INPUT NEW_DIR | guitar-variants INPUT CUES.json NEW_DIR all|VARIANT[,...] | ",
            "guitar-pitch-check REF.wav CAND.wav START_S END_S | ",
            "flute-layers|flute-faint|flute-cutoffs|flute-trace INPUT | verify-flute REPORT | ",
            "gesture|faint-export|accent-figure INPUT OUTPUT | cutoff-export|cue-passes INPUT NEW_DIR | ",
            "smoke INPUT_DIR | smoke-export INPUT_DIR NEW_DIR | six-samples|round2|round3|independent|masked-holdout PACK_ROOT | ",
            "sampler-run PACK_ROOT NEW_DIR plain|guard|shape | verify-corpus ROUND3 INDEPENDENT SIX | ",
            "capture-experiments|reference-match INPUT NEW_DIR | capture-figures DIR | response-figure OUTPUT.svg | reference-figures INPUT DIR"
        ).into()),
    }
}
