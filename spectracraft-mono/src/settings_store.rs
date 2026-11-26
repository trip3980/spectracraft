//! Thin compatibility wrapper around `craft-settings` (`storytold/craft-libs`).
//!
//! # Architectural Rule
//! All direct calls to `craft_settings` live in this module. The rest of SpectraCraft
//! interacts only with `SpectraCraftSettings`, `SettingsSnapshot`, and the helper functions
//! below. Unknown keys and comments in `config.toml` are preserved automatically by
//! `craft_settings` (`toml_edit::DocumentMut`) so future ArtCraft-wide schema additions
//! coexist without overwriting SpectraCraft's own keys.
//!
//! # Exact `craft-settings` API Surface Currently Consumed
//! - `craft_settings::Settings` trait (`APP_ID`, `SCHEMA_VERSION`, `migrate`)
//! - `craft_settings::Location` (`Location::Auto`, `Location::Dir`, `Location::Memory`)
//! - `craft_settings::SettingsStore` (`open`, `get`, `set`, `flush`, `path`, `is_read_only`)
//! - `craft_settings::DocumentMut` / `craft_settings::MigrateError`

use crate::Result;
use craft_settings::{DocumentMut, Location, MigrateError, Settings, SettingsStore};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Shared ArtCraft-scoped preferences. Kept in its own `[artcraft]` TOML table so
/// suite-wide schema updates can be adopted cleanly without rewriting SpectraCraft keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtCraftSharedPrefs {
    /// Suite-wide theme preference (`"dark"` default; `"light"` optional).
    pub theme: String,
    /// When a user accepts an update, automatically incorporate updates to shared ArtCraft
    /// crates already pinned by SpectraCraft (`craft-pdf`, `craft-atomic`, `craft-settings`)
    /// behind thin wrappers while leaving unused crates unlinked.
    pub auto_incorporate_shared_crates: bool,
}

impl Default for ArtCraftSharedPrefs {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            auto_incorporate_shared_crates: true,
        }
    }
}

/// Opt-in launch update check & user choice persistence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatePrefs {
    /// Whether to run a non-blocking background update check on launch.
    pub check_on_launch: bool,
    /// Check direct Cargo.toml dependencies in addition to craft-libs pins.
    pub check_direct_dependencies: bool,
    /// Last user choice: `"none" | "not_now" | "download_later" | "updated"`.
    pub last_action: String,
    /// Version notice fingerprint dismissed with "Not now" (cleared when a newer version appears).
    pub dismissed_notice_id: Option<String>,
    /// Revision or tag fetched via "Download and update later", awaiting explicit confirmation.
    pub pending_staged_target: Option<String>,
    /// Human-readable timestamp or note of last completed update.
    pub last_updated_at: Option<String>,
}

impl Default for UpdatePrefs {
    fn default() -> Self {
        Self {
            check_on_launch: true,
            check_direct_dependencies: true,
            last_action: "none".into(),
            dismissed_notice_id: None,
            pending_staged_target: None,
            last_updated_at: None,
        }
    }
}

/// Before/After technical report & viewer preferences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReportPrefs {
    /// Render the in-program technical report viewer in dark mode (default: true).
    pub dark_mode_viewer: bool,
    /// Default report export format: `"pdf" | "markdown" | "docx"`.
    pub default_export_format: String,
    /// Automatically open the report viewer once a blind report finishes generating.
    pub auto_open_viewer: bool,
}

impl Default for ReportPrefs {
    fn default() -> Self {
        Self {
            dark_mode_viewer: true,
            default_export_format: "pdf".into(),
            auto_open_viewer: true,
        }
    }
}

/// Local user-specific paths (stored only in local uncommitted `config.toml`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PathPrefs {
    pub last_input_dir: String,
    pub last_export_dir: String,
}

/// Optional integration metadata (stores only environment variable name, never raw secrets).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IntegrationPrefs {
    /// Environment variable inspected for optional AI-assist capability (never committed or echoed).
    pub ai_assist_env_var: String,
}

impl Default for IntegrationPrefs {
    fn default() -> Self {
        Self {
            ai_assist_env_var: "GEMINI_API_KEY".into(),
        }
    }
}

/// ArtCraft-style workspace panel & tool visibility preferences (`[workspace]`).
/// Calm Adobe/ArtCraft defaults: Left Tool Palette + Main Audio Canvas + Right Tabbed Panel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePrefs {
    pub show_waveform: bool,
    pub show_spectrogram: bool,
    pub show_inspector: bool,
    pub show_right_panel: bool,
    pub right_panel_tab: String,
    pub show_room_views: bool,
    pub show_diagnostics_dock: bool,
    pub show_tool_rail: bool,
    pub show_transport: bool,
    pub spatial_mode: String,
}

impl Default for WorkspacePrefs {
    fn default() -> Self {
        Self {
            show_waveform: true,
            show_spectrogram: true,
            show_inspector: true,
            show_right_panel: true,
            right_panel_tab: "inspector".into(),
            show_room_views: false,
            show_diagnostics_dock: false,
            show_tool_rail: true,
            show_transport: false,
            spatial_mode: "plans".into(),
        }
    }
}

/// Top-level versioned ArtCraft settings document for SpectraCraft (`config.toml`).
/// Intentionally omits `#[serde(deny_unknown_fields)]` per `craft-settings` convention so
/// future ArtCraft-wide keys survive untouched.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SpectraCraftSettings {
    pub artcraft: ArtCraftSharedPrefs,
    pub updates: UpdatePrefs,
    pub report: ReportPrefs,
    pub workspace: WorkspacePrefs,
    pub paths: PathPrefs,
    pub integration: IntegrationPrefs,
}

impl SpectraCraftSettings {
    /// Clamp and normalize user-editable fields to valid documented ranges.
    pub fn sanitize(&mut self) {
        self.artcraft.theme = match self.artcraft.theme.to_ascii_lowercase().as_str() {
            "light" => "light".into(),
            _ => "dark".into(),
        };
        self.updates.last_action = match self.updates.last_action.to_ascii_lowercase().as_str() {
            "not_now" => "not_now".into(),
            "download_later" => "download_later".into(),
            "updated" => "updated".into(),
            _ => "none".into(),
        };
        self.report.default_export_format = match self
            .report
            .default_export_format
            .to_ascii_lowercase()
            .as_str()
        {
            "markdown" | "md" => "markdown".into(),
            "docx" => "docx".into(),
            _ => "pdf".into(),
        };
        let env_clean: String = self
            .integration
            .ai_assist_env_var
            .trim()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(64)
            .collect();
        self.integration.ai_assist_env_var = if is_allowed_ai_assist_env_var(&env_clean) {
            env_clean
        } else {
            "GEMINI_API_KEY".into()
        };
        self.workspace.spatial_mode =
            match self.workspace.spatial_mode.to_ascii_lowercase().as_str() {
                "3d" => "3d".into(),
                "split" => "split".into(),
                _ => "plans".into(),
            };
        self.workspace.right_panel_tab =
            match self.workspace.right_panel_tab.to_ascii_lowercase().as_str() {
                "diagnostics" => "diagnostics".into(),
                "report" => "report".into(),
                "room" => "room".into(),
                _ => "inspector".into(),
            };
        self.paths.last_input_dir = sanitize_path_pref(&self.paths.last_input_dir);
        self.paths.last_export_dir = sanitize_path_pref(&self.paths.last_export_dir);
    }
}

fn sanitize_path_pref(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(512)
        .collect()
}

impl Settings for SpectraCraftSettings {
    const APP_ID: &'static str = "spectracraft";
    const SCHEMA_VERSION: u32 = 1;

    fn migrate(from: u32, _doc: &mut DocumentMut) -> std::result::Result<(), MigrateError> {
        let _ = from;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SettingsSnapshot {
    pub app_id: &'static str,
    pub schema_version: u32,
    pub backend_crate: &'static str,
    pub config_path: Option<String>,
    pub location_rule: String,
    pub load_source: String,
    pub read_only: bool,
    pub ai_assist_key_present: bool,
    pub settings: SpectraCraftSettings,
}

fn resolve_location(dir_override: Option<&Path>) -> Location {
    match dir_override {
        Some(dir) => Location::Dir(dir.to_path_buf()),
        None => Location::Auto,
    }
}

/// Allowlist of host environment variable names that SpectraCraft is permitted to
/// inspect for presence (boolean only; values are never read or serialized).
pub fn is_allowed_ai_assist_env_var(env_var: &str) -> bool {
    matches!(
        env_var,
        "GEMINI_API_KEY" | "GOOGLE_API_KEY" | "OPENAI_API_KEY" | "ANTHROPIC_API_KEY"
    ) || (env_var.starts_with("SPECTRACRAFT_")
        && env_var.len() > "SPECTRACRAFT_".len()
        && env_var.len() <= 64
        && env_var
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'))
}

fn check_env_key_present(env_var: &str) -> bool {
    is_allowed_ai_assist_env_var(env_var)
        && std::env::var(env_var)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
}

fn snapshot_from_store(
    store: &SettingsStore<SpectraCraftSettings>,
    rule: String,
    source: String,
) -> SettingsSnapshot {
    let settings = store.get().clone();
    let ai_assist_key_present = check_env_key_present(&settings.integration.ai_assist_env_var);
    SettingsSnapshot {
        app_id: SpectraCraftSettings::APP_ID,
        schema_version: SpectraCraftSettings::SCHEMA_VERSION,
        backend_crate: "craft-settings (storytold/craft-libs, isolated in settings_store.rs)",
        config_path: store.path().map(|p| p.display().to_string()),
        location_rule: rule,
        load_source: source,
        read_only: store.is_read_only(),
        ai_assist_key_present,
        settings,
    }
}

/// Load current SpectraCraft settings from the local uncommitted `craft-settings` store.
pub fn load_settings(dir_override: Option<&Path>) -> SettingsSnapshot {
    let (store, report) =
        SettingsStore::<SpectraCraftSettings>::open(resolve_location(dir_override));
    snapshot_from_store(&store, report.rule, format!("{:?}", report.source))
}

/// Atomically persist updated settings via `craft-settings` (preserving unknown ArtCraft keys).
pub fn save_settings(
    dir_override: Option<&Path>,
    mut next: SpectraCraftSettings,
) -> Result<SettingsSnapshot> {
    next.sanitize();
    let (mut store, report) =
        SettingsStore::<SpectraCraftSettings>::open(resolve_location(dir_override));
    store.set(next);
    store.flush()?;
    Ok(snapshot_from_store(
        &store,
        report.rule,
        format!("{:?}", report.source),
    ))
}

/// Apply an in-place mutation to persisted settings atomically.
pub fn mutate_settings(
    dir_override: Option<&Path>,
    f: impl FnOnce(&mut SpectraCraftSettings),
) -> Result<SettingsSnapshot> {
    let (mut store, report) =
        SettingsStore::<SpectraCraftSettings>::open(resolve_location(dir_override));
    let mut next = store.get().clone();
    f(&mut next);
    next.sanitize();
    store.set(next);
    store.flush()?;
    Ok(snapshot_from_store(
        &store,
        report.rule,
        format!("{:?}", report.source),
    ))
}

/// Reset all SpectraCraft settings to documented defaults and flush atomically.
pub fn reset_settings(dir_override: Option<&Path>) -> Result<SettingsSnapshot> {
    save_settings(dir_override, SpectraCraftSettings::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_settings_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "spectracraft_settings_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn settings_survive_restart_and_reset_restores_defaults() {
        let dir = temp_settings_dir("roundtrip");
        let initial = load_settings(Some(&dir));
        assert!(initial.settings.report.dark_mode_viewer);
        assert!(initial.settings.updates.check_on_launch);
        assert_eq!(initial.settings.report.default_export_format, "pdf");
        assert!(initial.settings.workspace.show_waveform);
        assert!(initial.settings.workspace.show_spectrogram);
        assert!(initial.settings.workspace.show_tool_rail);
        assert!(initial.settings.workspace.show_right_panel);
        assert_eq!(initial.settings.workspace.right_panel_tab, "inspector");
        assert!(!initial.settings.workspace.show_room_views);
        assert!(!initial.settings.workspace.show_diagnostics_dock);

        let mut edited = initial.settings.clone();
        edited.report.dark_mode_viewer = false;
        edited.report.default_export_format = "markdown".into();
        edited.updates.check_on_launch = false;
        edited.updates.dismissed_notice_id = Some("craft-libs:v0.2.0".into());
        edited.workspace.show_tool_rail = false;
        edited.workspace.show_right_panel = false;
        edited.workspace.right_panel_tab = "report".into();
        edited.workspace.show_room_views = true;
        edited.workspace.show_diagnostics_dock = true;
        edited.workspace.show_transport = true;
        edited.workspace.spatial_mode = "split".into();
        edited.paths.last_export_dir = "/tmp/exports".into();
        save_settings(Some(&dir), edited.clone()).unwrap();

        // Reopen fresh store to verify persistence across restarts.
        let reloaded = load_settings(Some(&dir));
        assert_eq!(reloaded.settings, edited);

        // Reset to defaults restores sensible documented defaults.
        let reset = reset_settings(Some(&dir)).unwrap();
        assert_eq!(reset.settings, SpectraCraftSettings::default());
        assert!(reset.settings.report.dark_mode_viewer);
        assert!(reset.settings.workspace.show_tool_rail);
        assert!(reset.settings.workspace.show_right_panel);
        assert_eq!(reset.settings.workspace.right_panel_tab, "inspector");
        assert!(!reset.settings.workspace.show_room_views);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_artcraft_suite_keys_survive_spectracraft_writes() {
        let dir = temp_settings_dir("preserve_unknown");
        let cfg = dir.join("config.toml");
        crate::report_export::atomic_write_bytes(
            &cfg,
            b"schema_version = 1\n\n[artcraft_future]\nshared_palette = \"studio-midnight\"\n\n[report]\ndark_mode_viewer = true\n",
        )
        .unwrap();

        mutate_settings(Some(&dir), |s| {
            s.report.default_export_format = "markdown".into();
        })
        .unwrap();

        let raw = std::fs::read_to_string(&cfg).unwrap();
        assert!(
            raw.contains("shared_palette = \"studio-midnight\""),
            "Unknown ArtCraft keys must survive round-trip: {raw}"
        );
        assert!(raw.contains("default_export_format = \"markdown\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_blocks_arbitrary_env_probing_and_handles_multibyte_utf8_paths() {
        assert!(is_allowed_ai_assist_env_var("GEMINI_API_KEY"));
        assert!(is_allowed_ai_assist_env_var("SPECTRACRAFT_AI_KEY"));
        assert!(!is_allowed_ai_assist_env_var("PATH"));
        assert!(!is_allowed_ai_assist_env_var("AWS_SECRET_ACCESS_KEY"));
        assert!(!check_env_key_present("PATH"));

        let mut s = SpectraCraftSettings::default();
        s.integration.ai_assist_env_var = "PATH".into();
        // 300 3-byte UTF-8 chars (900 bytes > 512) + control chars: must not panic
        s.paths.last_input_dir = format!("/tmp/\n\r{}", "音".repeat(600));
        s.paths.last_export_dir = format!("/tmp/\0{}", "♪".repeat(600));
        s.sanitize();

        assert_eq!(s.integration.ai_assist_env_var, "GEMINI_API_KEY");
        assert_eq!(s.paths.last_input_dir.chars().count(), 512);
        assert!(!s.paths.last_input_dir.chars().any(|c| c.is_control()));
        assert_eq!(s.paths.last_export_dir.chars().count(), 512);
        assert!(!s.paths.last_export_dir.chars().any(|c| c.is_control()));
    }
}
