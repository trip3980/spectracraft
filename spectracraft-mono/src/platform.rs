//! Small cross-platform compatibility layer for SpectraCraft.
//!
//! Responsibilities isolated here:
//! - Always-available host metadata & portable path helpers (Linux, Windows, macOS, and future mobile targets).
//! - Optional platform adapters behind fine-grained features (`desktop` enables all three):
//!   - Native file dialogs (`#[cfg(feature = "dialogs")]`, `rfd`: Linux xdg-portal, Windows IFileDialog, macOS NSOpenPanel/NSSavePanel)
//!   - Native audio output device (`#[cfg(feature = "playback")]`, `rodio` / `cpal`: Linux ALSA, Windows WASAPI, macOS CoreAudio)
//!   - Native egui windowing (`#[cfg(feature = "desktop")]`, `eframe` / `winit` / `glow`: Linux X11, Windows Win32/WGL, macOS AppKit/NSOpenGL)
//!
//! Core audio stages (dry regeneration, surgical region editing, microphone, room, plugin slot)
//! MUST NOT depend on desktop platform APIs.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Target operating system family across desktop and mobile platforms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPlatform {
    Linux,
    Windows,
    MacOs,
    Ios,
    Android,
    Other,
}

impl HostPlatform {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::Windows => "Windows",
            Self::MacOs => "macOS",
            Self::Ios => "iOS",
            Self::Android => "Android",
            Self::Other => "Other",
        }
    }

    pub const fn is_mobile(self) -> bool {
        matches!(self, Self::Ios | Self::Android)
    }

    /// Map a Rust `std::env::consts::OS` string to a typed `HostPlatform`.
    pub fn for_target_os(os: &str) -> Self {
        match os {
            "linux" => Self::Linux,
            "windows" => Self::Windows,
            "macos" => Self::MacOs,
            "ios" => Self::Ios,
            "android" => Self::Android,
            _ => Self::Other,
        }
    }

    /// True when native Zed `gpui_platform` supports direct GPU windowing on this OS.
    pub const fn supports_native_gpui(self) -> bool {
        matches!(self, Self::Linux | Self::Windows | Self::MacOs)
    }
}

/// Native frontend shell connected to the shared `spectracraft-mono` Rust engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendShell {
    GpuiDesktop,
    EguiDesktop,
    TauriMobile,
    TauriDesktop,
    WebBridge,
}

impl FrontendShell {
    pub const fn label(self) -> &'static str {
        match self {
            Self::GpuiDesktop => "Native GPUI Desktop",
            Self::EguiDesktop => "Native egui Desktop",
            Self::TauriMobile => "Tauri 2 Mobile (Shared Rust Core)",
            Self::TauriDesktop => "Tauri 2 Desktop (Shared Rust Core)",
            Self::WebBridge => "Express / Browser Bridge",
        }
    }
}

/// Authoritative runtime capability summary shared across GPUI, egui, Tauri, and CLI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformCapabilities {
    pub platform: HostPlatform,
    pub os: String,
    pub arch: String,
    pub label: String,
    pub mobile: bool,
    pub supports_native_gpui: bool,
    pub recommended_shell: FrontendShell,
    pub gpu_backend: String,
    pub audio_backend: String,
    pub app_data_dir: String,
    pub temp_dir: String,
}

/// Declarative cross-platform delivery entry for Windows, Linux, macOS, iOS, and Android.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformTargetSpec {
    pub platform: HostPlatform,
    pub target_triples: Vec<String>,
    pub primary_shell: FrontendShell,
    pub secondary_shells: Vec<FrontendShell>,
    pub supports_native_gpui: bool,
    pub gpu_backend: String,
    pub audio_backend: String,
    pub shared_preset_format: String,
    pub notes: String,
}

/// Returns the current host OS family determined at compile time.
pub const fn current_platform() -> HostPlatform {
    if cfg!(target_os = "linux") {
        HostPlatform::Linux
    } else if cfg!(target_os = "windows") {
        HostPlatform::Windows
    } else if cfg!(target_os = "macos") {
        HostPlatform::MacOs
    } else if cfg!(target_os = "ios") {
        HostPlatform::Ios
    } else if cfg!(target_os = "android") {
        HostPlatform::Android
    } else {
        HostPlatform::Other
    }
}

/// Recommended GPU backend descriptor for `platform`.
pub const fn gpu_backend_for(platform: HostPlatform) -> &'static str {
    match platform {
        HostPlatform::Windows => "DirectX 11/12 (GPUI) / WebView2 (Tauri)",
        HostPlatform::Linux => "Vulkan / Wayland / X11 (GPUI) / WebKitGTK (Tauri)",
        HostPlatform::MacOs => "Metal (GPUI) / WKWebView (Tauri)",
        HostPlatform::Ios => "WKWebView / Metal (Tauri 2 iOS; gpui_platform is desktop-only)",
        HostPlatform::Android => "Android WebView (Tauri 2 Android; gpui_platform is desktop-only)",
        HostPlatform::Other => "Headless / Portable Core",
    }
}

/// Recommended audio backend descriptor for `platform`.
pub const fn audio_backend_for(platform: HostPlatform) -> &'static str {
    match platform {
        HostPlatform::Windows => "WASAPI (rodio / cpal)",
        HostPlatform::Linux => "ALSA (rodio / cpal)",
        HostPlatform::MacOs => "CoreAudio (rodio / cpal)",
        HostPlatform::Ios => "AVAudioSession / WebAudio (Tauri 2 iOS)",
        HostPlatform::Android => "AAudio / Oboe / WebAudio (Tauri 2 Android)",
        HostPlatform::Other => "Offline WAV / Portable Core",
    }
}

/// Recommended primary native shell for `platform`.
pub const fn recommended_shell_for(platform: HostPlatform) -> FrontendShell {
    if platform.supports_native_gpui() {
        FrontendShell::GpuiDesktop
    } else if platform.is_mobile() {
        FrontendShell::TauriMobile
    } else {
        FrontendShell::WebBridge
    }
}

/// Returns the current host's full cross-platform & GPUI capability descriptor.
pub fn current_capabilities() -> PlatformCapabilities {
    let platform = current_platform();
    PlatformCapabilities {
        platform,
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        label: platform.label().to_owned(),
        mobile: platform.is_mobile(),
        supports_native_gpui: platform.supports_native_gpui(),
        recommended_shell: recommended_shell_for(platform),
        gpu_backend: gpu_backend_for(platform).to_owned(),
        audio_backend: audio_backend_for(platform).to_owned(),
        app_data_dir: app_data_dir().display().to_string(),
        temp_dir: temp_dir().display().to_string(),
    }
}

/// Returns the canonical 5-platform delivery matrix connecting Windows, Linux, macOS, iOS, and Android
/// to the shared `spectracraft-mono` core engine and GPUI/Tauri shells.
pub fn supported_platform_matrix() -> Vec<PlatformTargetSpec> {
    let preset_fmt = "spectracraft_mono::app_session::SessionPreset (JSON v1)".to_owned();
    vec![
        PlatformTargetSpec {
            platform: HostPlatform::Windows,
            target_triples: vec!["x86_64-pc-windows-msvc".to_owned()],
            primary_shell: FrontendShell::GpuiDesktop,
            secondary_shells: vec![FrontendShell::TauriDesktop, FrontendShell::EguiDesktop],
            supports_native_gpui: true,
            gpu_backend: gpu_backend_for(HostPlatform::Windows).to_owned(),
            audio_backend: audio_backend_for(HostPlatform::Windows).to_owned(),
            shared_preset_format: preset_fmt.clone(),
            notes: "Native GPUI desktop (.exe/zip) + Tauri 2 MSI/NSIS + CLI.".to_owned(),
        },
        PlatformTargetSpec {
            platform: HostPlatform::Linux,
            target_triples: vec![
                "x86_64-unknown-linux-gnu".to_owned(),
                "aarch64-unknown-linux-gnu".to_owned(),
            ],
            primary_shell: FrontendShell::GpuiDesktop,
            secondary_shells: vec![FrontendShell::TauriDesktop, FrontendShell::EguiDesktop],
            supports_native_gpui: true,
            gpu_backend: gpu_backend_for(HostPlatform::Linux).to_owned(),
            audio_backend: audio_backend_for(HostPlatform::Linux).to_owned(),
            shared_preset_format: preset_fmt.clone(),
            notes: "Native GPUI desktop (Wayland/X11/Vulkan) + Tauri 2 deb/AppImage + CLI."
                .to_owned(),
        },
        PlatformTargetSpec {
            platform: HostPlatform::MacOs,
            target_triples: vec![
                "aarch64-apple-darwin".to_owned(),
                "x86_64-apple-darwin".to_owned(),
            ],
            primary_shell: FrontendShell::GpuiDesktop,
            secondary_shells: vec![FrontendShell::TauriDesktop, FrontendShell::EguiDesktop],
            supports_native_gpui: true,
            gpu_backend: gpu_backend_for(HostPlatform::MacOs).to_owned(),
            audio_backend: audio_backend_for(HostPlatform::MacOs).to_owned(),
            shared_preset_format: preset_fmt.clone(),
            notes: "Native GPUI .app bundle (Metal + CoreAudio) + Tauri 2 DMG/Universal + CLI."
                .to_owned(),
        },
        PlatformTargetSpec {
            platform: HostPlatform::Ios,
            target_triples: vec![
                "aarch64-apple-ios".to_owned(),
                "aarch64-apple-ios-sim".to_owned(),
            ],
            primary_shell: FrontendShell::TauriMobile,
            secondary_shells: vec![],
            supports_native_gpui: false,
            gpu_backend: gpu_backend_for(HostPlatform::Ios).to_owned(),
            audio_backend: audio_backend_for(HostPlatform::Ios).to_owned(),
            shared_preset_format: preset_fmt.clone(),
            notes: "Tauri 2 iOS shell over shared spectracraft-mono core (--no-default-features); upstream gpui_platform does not provide a UIKit backend."
                .to_owned(),
        },
        PlatformTargetSpec {
            platform: HostPlatform::Android,
            target_triples: vec![
                "aarch64-linux-android".to_owned(),
                "armv7-linux-androideabi".to_owned(),
                "x86_64-linux-android".to_owned(),
            ],
            primary_shell: FrontendShell::TauriMobile,
            secondary_shells: vec![],
            supports_native_gpui: false,
            gpu_backend: gpu_backend_for(HostPlatform::Android).to_owned(),
            audio_backend: audio_backend_for(HostPlatform::Android).to_owned(),
            shared_preset_format: preset_fmt,
            notes: "Tauri 2 Android shell over shared spectracraft-mono core (--no-default-features); upstream gpui_platform does not provide an Android surface backend."
                .to_owned(),
        },
    ]
}

/// Serialize `current_capabilities()` and `supported_platform_matrix()` to pretty JSON.
pub fn platform_report_json() -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "current": current_capabilities(),
        "targets": supported_platform_matrix(),
    }))
    .unwrap_or_else(|_| "{}".to_owned())
}

/// Portable OS temporary directory (never hardcodes `/tmp`).
pub fn temp_dir() -> PathBuf {
    std::env::temp_dir()
}

/// Best-effort user home directory across Linux, macOS, Windows, iOS, and Android without external crates.
pub fn user_home_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        if let Some(p) = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(p));
        }
        if let (Some(drive), Some(path)) =
            (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH"))
        {
            let mut combined = drive;
            combined.push(path);
            if !combined.is_empty() {
                return Some(PathBuf::from(combined));
            }
        }
        None
    } else {
        std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    }
}

/// Standard per-OS user application data directory for SpectraCraft sessions and preferences.
pub fn app_data_dir() -> PathBuf {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        if let Some(home) = user_home_dir() {
            return home
                .join("Library")
                .join("Application Support")
                .join("SpectraCraft");
        }
    } else if cfg!(target_os = "windows") {
        if let Some(appdata) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) {
            return PathBuf::from(appdata).join("SpectraCraft");
        }
        if let Some(home) = user_home_dir() {
            return home.join("AppData").join("Roaming").join("SpectraCraft");
        }
    } else {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            return PathBuf::from(xdg).join("spectracraft");
        }
        if let Some(home) = user_home_dir() {
            return home.join(".local").join("share").join("spectracraft");
        }
    }
    temp_dir().join("spectracraft")
}

/// Normalizes a path pasted or typed by a user across Windows, macOS, and Linux:
/// - trims surrounding whitespace
/// - strips matching surrounding single or double quotes (e.g. Windows Explorer "Copy as path")
/// - expands a leading `~/` or `~\` against `user_home_dir()`
pub fn normalize_user_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    let unquoted = if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    if let Some(rest) = unquoted
        .strip_prefix("~/")
        .or_else(|| unquoted.strip_prefix("~\\"))
    {
        if let Some(home) = user_home_dir() {
            return home.join(rest);
        }
    } else if unquoted == "~" {
        if let Some(home) = user_home_dir() {
            return home;
        }
    }
    PathBuf::from(unquoted)
}

/// Platform-appropriate Undo shortcut label for desktop menus.
pub const fn undo_shortcut_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Undo  (Cmd+Z)"
    } else {
        "Undo  (Ctrl+Z)"
    }
}

/// Platform-appropriate Redo shortcut label for desktop menus.
pub const fn redo_shortcut_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Redo  (Cmd+Shift+Z)"
    } else {
        "Redo  (Ctrl+Shift+Z)"
    }
}

/// Clamp an optional `[start_sample, end_sample)` audio playback/viewport slice safely within `total_len`.
pub fn clamp_playback_slice(
    total_len: usize,
    start_sample: usize,
    end_sample: Option<usize>,
) -> (usize, usize) {
    let end = end_sample.unwrap_or(total_len).min(total_len);
    let start = start_sample.min(end);
    (start, end)
}

/// Native desktop file dialogs (Linux xdg-portal, Windows IFileDialog, macOS NSOpenPanel/NSSavePanel).
#[cfg(feature = "dialogs")]
pub mod dialogs {
    use std::path::PathBuf;

    pub fn pick_wav_file() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("WAV", &["wav"])
            .pick_file()
    }

    pub fn save_wav_file(default_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_file_name(default_name)
            .add_filter("WAV", &["wav"])
            .save_file()
    }

    pub fn pick_session_file() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("SpectraCraft Session", &["json"])
            .pick_file()
    }

    pub fn save_session_file(default_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_file_name(default_name)
            .add_filter("SpectraCraft Session", &["json"])
            .save_file()
    }

    pub fn pick_analysis_file() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("Analysis", &["json"])
            .pick_file()
    }

    pub fn save_analysis_file(default_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_file_name(default_name)
            .add_filter("Analysis", &["json"])
            .save_file()
    }

    pub fn pick_export_folder() -> Option<PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }

    pub fn save_report_file(default_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_file_name(default_name)
            .add_filter("SpectraCraft Report (PDF / Markdown)", &["pdf", "md"])
            .save_file()
    }
}

/// Native desktop mono audio playback handle backed by `rodio` / `cpal`
/// (Linux ALSA, Windows WASAPI, macOS CoreAudio).
#[cfg(feature = "playback")]
pub struct AudioPlayback {
    _stream: rodio::OutputStream,
    sink: rodio::Sink,
    started_at: f64,
}

#[cfg(feature = "playback")]
impl AudioPlayback {
    /// Opens the default OS audio output device and starts mono playback at `sample_rate`.
    pub fn play_mono(
        sample_rate: u32,
        samples: Vec<f32>,
        started_at: f64,
    ) -> std::result::Result<Self, String> {
        let (stream, handle) = rodio::OutputStream::try_default().map_err(|e| format!("{e}"))?;
        let sink = rodio::Sink::try_new(&handle).map_err(|e| format!("{e}"))?;
        sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, samples));
        Ok(Self {
            _stream: stream,
            sink,
            started_at,
        })
    }

    /// Starts mono playback from only the requested `[start_sample, end_sample)` slice without copying the full track.
    pub fn play_mono_slice(
        sample_rate: u32,
        samples: &[f32],
        start_sample: usize,
        end_sample: Option<usize>,
        started_at: f64,
    ) -> std::result::Result<Self, String> {
        let (s0, s1) = clamp_playback_slice(samples.len(), start_sample, end_sample);
        Self::play_mono(sample_rate, samples[s0..s1].to_vec(), started_at)
    }

    /// Returns true once the queued audio buffer has finished playing.
    pub fn is_finished(&self) -> bool {
        self.sink.empty()
    }

    /// Returns the playback-owned timeline cursor position in seconds.
    pub fn position_secs(&self) -> f64 {
        self.started_at + self.sink.get_pos().as_secs_f64()
    }

    /// Stops playback and returns the final cursor position (falling back to `fallback_cursor` if finished).
    pub fn stop(self, fallback_cursor: f64) -> f64 {
        let pos = if self.sink.empty() {
            fallback_cursor
        } else {
            self.position_secs()
        };
        self.sink.stop();
        pos
    }
}

/// Launches the native desktop `eframe` window (Linux X11, Windows Win32/WGL, macOS AppKit/NSOpenGL).
#[cfg(feature = "desktop")]
pub fn run_native_window<F>(
    title: &str,
    default_size: [f32; 2],
    min_size: [f32; 2],
    app_creator: F,
) -> eframe::Result<()>
where
    F: FnOnce(
            &eframe::CreationContext<'_>,
        ) -> std::result::Result<
            Box<dyn eframe::App>,
            Box<dyn std::error::Error + Send + Sync>,
        > + 'static,
{
    eframe::run_native(
        title,
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size(default_size)
                .with_min_inner_size(min_size),
            ..Default::default()
        },
        Box::new(app_creator),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_user_path_handles_whitespace_and_windows_quotes() {
        assert_eq!(
            normalize_user_path("  \"C:\\Audio\\take 1.wav\"  "),
            PathBuf::from("C:\\Audio\\take 1.wav")
        );
        assert_eq!(
            normalize_user_path("'/Users/artist/take.wav'"),
            PathBuf::from("/Users/artist/take.wav")
        );
        assert_eq!(
            normalize_user_path("relative/take.wav"),
            PathBuf::from("relative/take.wav")
        );
    }

    #[test]
    fn normalize_user_path_expands_tilde_when_home_available() {
        if let Some(home) = user_home_dir() {
            assert_eq!(normalize_user_path("~/take.wav"), home.join("take.wav"));
            assert_eq!(normalize_user_path("~"), home);
        }
    }

    #[test]
    fn host_paths_and_shortcut_labels_are_populated() {
        assert!(!temp_dir().as_os_str().is_empty());
        assert!(!app_data_dir().as_os_str().is_empty());
        assert!(!current_platform().label().is_empty());
        assert!(undo_shortcut_label().contains("Undo"));
        assert!(redo_shortcut_label().contains("Redo"));
    }

    #[test]
    fn clamp_playback_slice_bounds_ranges_safely() {
        assert_eq!(clamp_playback_slice(1000, 250, None), (250, 1000));
        assert_eq!(clamp_playback_slice(1000, 250, Some(600)), (250, 600));
        assert_eq!(clamp_playback_slice(1000, 1500, Some(2000)), (1000, 1000));
        assert_eq!(clamp_playback_slice(1000, 800, Some(400)), (400, 400));
    }

    #[test]
    fn platform_matrix_connects_all_five_platforms_with_honest_gpui_and_mobile_shells() {
        let matrix = supported_platform_matrix();
        assert_eq!(matrix.len(), 5);
        for (os, expected_gpui, expected_mobile, expected_shell) in [
            ("windows", true, false, FrontendShell::GpuiDesktop),
            ("linux", true, false, FrontendShell::GpuiDesktop),
            ("macos", true, false, FrontendShell::GpuiDesktop),
            ("ios", false, true, FrontendShell::TauriMobile),
            ("android", false, true, FrontendShell::TauriMobile),
        ] {
            let p = HostPlatform::for_target_os(os);
            assert_eq!(p.supports_native_gpui(), expected_gpui);
            assert_eq!(p.is_mobile(), expected_mobile);
            assert_eq!(recommended_shell_for(p), expected_shell);
            let spec = matrix
                .iter()
                .find(|entry| entry.platform == p)
                .expect("target platform present in matrix");
            assert!(!spec.target_triples.is_empty());
            assert_eq!(spec.supports_native_gpui, expected_gpui);
            assert_eq!(spec.primary_shell, expected_shell);
        }

        let report_json = platform_report_json();
        let parsed: serde_json::Value = serde_json::from_str(&report_json).unwrap();
        assert_eq!(parsed["targets"].as_array().map(|a| a.len()), Some(5));
        assert!(!parsed["current"]["os"].as_str().unwrap_or("").is_empty());
    }
}
