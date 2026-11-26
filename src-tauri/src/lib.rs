//! Thin Tauri 2 desktop and mobile shell over the unchanged `spectracraft-mono` core.
//!
//! All five-stage DSP physics, blind Before/After reporting, settings persistence,
//! and DAW interchange stay inside `spectracraft-mono`; this crate only exposes
//! thin IPC commands to the React + Vite UI.

use serde::Serialize;
use spectracraft_mono::{
    daw_bridge::{
        export_for_editor, markers_from_performance, EditorTarget, ExportBitDepth,
        InterchangeExportRequest, InterchangeReceipt, StemTrack,
    },
    pipeline::AudioPipelineEngine,
    platform, qa,
    reperform::Performance,
    settings_store::{load_settings, reset_settings, save_settings, SpectraCraftSettings},
};
use std::path::{Component, Path, PathBuf};

const MAX_IPC_PATH_LEN: usize = 1024;
const MAX_IPC_TITLE_LEN: usize = 160;

fn validate_ipc_path_basics(raw: &str, label: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} path is required"));
    }
    if trimmed.len() > MAX_IPC_PATH_LEN {
        return Err(format!("{label} path exceeds maximum allowed length"));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(format!("{label} path contains invalid control characters"));
    }
    let path = Path::new(trimmed);
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("{label} path must not contain '..' traversal segments"));
    }
    Ok(path.to_path_buf())
}

fn validate_ipc_input_file(raw: &str, expected_ext: &str, label: &str) -> Result<PathBuf, String> {
    let path = validate_ipc_path_basics(raw, label)?;
    let ext_ok = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(expected_ext))
        .unwrap_or(false);
    if !ext_ok {
        return Err(format!("{label} must be a .{expected_ext} file"));
    }
    let meta = std::fs::metadata(&path)
        .map_err(|_| format!("{label} file does not exist or is unreadable"))?;
    if !meta.is_file() {
        return Err(format!("{label} must point to a regular file"));
    }
    std::fs::canonicalize(&path).map_err(|_| format!("Unable to resolve {label} file"))
}

fn validate_ipc_output_dir(raw: &str) -> Result<PathBuf, String> {
    let path = validate_ipc_path_basics(raw, "Export output directory")?;
    if path.file_name().is_none() {
        return Err("Export output directory cannot be a filesystem root".into());
    }
    if path.exists() && !path.is_dir() {
        return Err("Export output path exists and is not a directory".into());
    }
    Ok(path)
}

fn sanitize_ipc_title(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_IPC_TITLE_LEN)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "SpectraCraft Session".to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Serialize)]
struct PlatformInfo {
    os: &'static str,
    arch: &'static str,
    mobile: bool,
    app_data_dir: String,
    temp_dir: String,
}

#[derive(Serialize)]
struct DawTargetInfo {
    id: &'static str,
    name: &'static str,
}

#[tauri::command]
fn platform_info() -> PlatformInfo {
    PlatformInfo {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        mobile: cfg!(any(target_os = "ios", target_os = "android")),
        app_data_dir: platform::app_data_dir().display().to_string(),
        temp_dir: platform::temp_dir().display().to_string(),
    }
}

#[tauri::command]
fn supported_daw_targets() -> Vec<DawTargetInfo> {
    EditorTarget::ALL
        .iter()
        .map(|t| DawTargetInfo {
            id: t.id(),
            name: t.display_name(),
        })
        .collect()
}

#[tauri::command]
fn get_settings() -> SpectraCraftSettings {
    load_settings(None)
}

#[tauri::command]
fn update_settings(next: SpectraCraftSettings) -> Result<SpectraCraftSettings, String> {
    save_settings(None, next).map_err(|_| "Failed to save settings".to_string())
}

#[tauri::command]
fn reset_settings_defaults() -> Result<SpectraCraftSettings, String> {
    reset_settings(None).map_err(|_| "Failed to reset settings".to_string())
}

#[tauri::command]
fn export_daw_interchange(
    target_id: String,
    dry_wav: String,
    final_wav: String,
    out_dir: String,
    cues_json: Option<String>,
    session_title: String,
    bit_depth: Option<String>,
) -> Result<InterchangeReceipt, String> {
    let target = EditorTarget::from_id(&target_id)
        .ok_or_else(|| "Unsupported DAW target (expected audition, logic, daw, or soundcraft)".to_string())?;
    let depth = ExportBitDepth::from_id(bit_depth.as_deref().unwrap_or("24"))
        .ok_or_else(|| "Unsupported bit depth (expected 24 or 32)".to_string())?;

    let safe_dry_wav = validate_ipc_input_file(&dry_wav, "wav", "Dry stem")?;
    let safe_final_wav = validate_ipc_input_file(&final_wav, "wav", "Final stem")?;
    let safe_out_dir = validate_ipc_output_dir(&out_dir)?;
    let safe_title = sanitize_ipc_title(&session_title);

    let (dry_spec, dry_samples) = qa::read_audio(&safe_dry_wav)
        .map_err(|_| "Failed to read dry WAV stem".to_string())?;
    let (final_spec, final_samples) = qa::read_audio(&safe_final_wav)
        .map_err(|_| "Failed to read final WAV stem".to_string())?;
    if dry_spec.channels != 1 || final_spec.channels != 1 {
        return Err("Expected mono WAV stems for DAW interchange export".into());
    }
    if dry_spec.sample_rate != final_spec.sample_rate {
        return Err(format!(
            "Sample rate mismatch: dry {} Hz vs final {} Hz",
            dry_spec.sample_rate, final_spec.sample_rate
        ));
    }

    let markers = match cues_json {
        Some(path) if !path.trim().is_empty() => {
            let safe_cues = validate_ipc_input_file(&path, "json", "Performance cues")?;
            let file = std::fs::File::open(&safe_cues)
                .map_err(|_| "Failed to open performance cues JSON".to_string())?;
            let perf: Performance = serde_json::from_reader(file)
                .map_err(|_| "Failed to parse performance cues JSON".to_string())?;
            markers_from_performance(&perf)
        }
        _ => Vec::new(),
    };

    let engine = AudioPipelineEngine::new(dry_spec.sample_rate)
        .map_err(|_| "Failed to initialize audio pipeline state snapshot".to_string())?;
    let snapshot = engine.snapshot_state();
    let stems = [
        StemTrack {
            name: "Dry Take (Stage 1)",
            slug: "dry-take",
            samples: &dry_samples,
        },
        StemTrack {
            name: "Final Take (Stages 3-5)",
            slug: "final-take",
            samples: &final_samples,
        },
    ];

    let req = InterchangeExportRequest {
        session_title: &safe_title,
        sample_rate: dry_spec.sample_rate,
        bit_depth: depth,
        target,
        stems: &stems,
        markers: &markers,
        engine_state: Some(&snapshot),
        source_signature_note: None,
    };

    export_for_editor(&safe_out_dir, &req)
        .map_err(|_| "Failed to export DAW interchange bundle".to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            platform_info,
            supported_daw_targets,
            get_settings,
            update_settings,
            reset_settings_defaults,
            export_daw_interchange,
        ])
        .run(tauri::generate_context!())
        .expect("error while running SpectraCraft Tauri shell");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_path_validation_blocks_traversal_control_chars_and_bad_extensions() {
        assert!(validate_ipc_input_file("../secret.wav", "wav", "Dry stem").is_err());
        assert!(validate_ipc_input_file("/tmp/a\0b.wav", "wav", "Dry stem").is_err());
        assert!(validate_ipc_input_file("/etc/passwd", "wav", "Dry stem").is_err());
        assert!(validate_ipc_output_dir("/").is_err());
        assert!(validate_ipc_output_dir("/tmp/../etc").is_err());
        assert_eq!(sanitize_ipc_title("Solo\r\nGuitar\x00"), "SoloGuitar");
        assert_eq!(sanitize_ipc_title("   \n\t "), "SpectraCraft Session");
    }
}
