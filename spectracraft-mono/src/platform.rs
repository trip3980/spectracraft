//! Small cross-platform compatibility layer for SpectraCraft.
//!
//! Responsibilities isolated here:
//! - Always-available host metadata & portable path helpers (Linux, Windows, macOS, and future mobile targets).
//! - Desktop-only (`#[cfg(feature = "desktop")]`) adapters for:
//!   - Native file dialogs (`rfd`: Linux xdg-portal, Windows IFileDialog, macOS NSOpenPanel/NSSavePanel)
//!   - Native audio output device (`rodio` / `cpal`: Linux ALSA, Windows WASAPI, macOS CoreAudio)
//!   - Native windowing (`eframe` / `winit` / `glow`: Linux X11, Windows Win32/WGL, macOS AppKit/NSOpenGL)
//!
//! Core audio stages (dry regeneration, surgical region editing, microphone, room, plugin slot)
//! MUST NOT depend on desktop platform APIs.

use std::path::PathBuf;

/// Target desktop/host operating system family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostPlatform {
    Linux,
    Windows,
    MacOs,
    Other,
}

impl HostPlatform {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::Windows => "Windows",
            Self::MacOs => "macOS",
            Self::Other => "Other",
        }
    }
}

/// Returns the current host OS family determined at compile time.
pub const fn current_platform() -> HostPlatform {
    if cfg!(target_os = "linux") {
        HostPlatform::Linux
    } else if cfg!(target_os = "windows") {
        HostPlatform::Windows
    } else if cfg!(target_os = "macos") {
        HostPlatform::MacOs
    } else {
        HostPlatform::Other
    }
}

/// Portable OS temporary directory (never hardcodes `/tmp`).
pub fn temp_dir() -> PathBuf {
    std::env::temp_dir()
}

/// Best-effort user home directory across Linux, macOS, and Windows without external crates.
pub fn user_home_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        if let Some(p) = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(p));
        }
        if let (Some(drive), Some(path)) = (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
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
    if cfg!(target_os = "macos") {
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

/// Native desktop file dialogs (Linux xdg-portal, Windows IFileDialog, macOS NSOpenPanel/NSSavePanel).
#[cfg(feature = "desktop")]
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
}

/// Native desktop mono audio playback handle backed by `rodio` / `cpal`
/// (Linux ALSA, Windows WASAPI, macOS CoreAudio).
#[cfg(feature = "desktop")]
pub struct AudioPlayback {
    _stream: rodio::OutputStream,
    sink: rodio::Sink,
    started_at: f64,
}

#[cfg(feature = "desktop")]
impl AudioPlayback {
    /// Opens the default OS audio output device and starts mono playback at `sample_rate`.
    pub fn play_mono(
        sample_rate: u32,
        samples: Vec<f32>,
        started_at: f64,
    ) -> std::result::Result<Self, String> {
        let (stream, handle) =
            rodio::OutputStream::try_default().map_err(|e| format!("{e}"))?;
        let sink = rodio::Sink::try_new(&handle).map_err(|e| format!("{e}"))?;
        sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, samples));
        Ok(Self {
            _stream: stream,
            sink,
            started_at,
        })
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
    F: FnOnce(&eframe::CreationContext<'_>) -> std::result::Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>>
        + 'static,
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
}
