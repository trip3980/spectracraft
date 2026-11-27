//! Cross-platform native GPUI desktop entry point for SpectraCraft (Windows, macOS, Linux).
//!
//! Launches a standalone GPUI workstation backed by shared `spectracraft-mono`
//! session, worker, visualization, blind reporting, DAW interchange, and playback operations.
//! Mobile targets (iOS / Android) connect to the exact same `spectracraft-mono` engine and
//! `SessionPreset` format through the Tauri 2 shell (`src-tauri/`).

#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod audio_canvas;
mod bridge;
mod inspector;
mod transport;

use app::{SpectraCraftGpuiApp, register_keybindings};
use gpui::{App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use gpui_platform::application;
use spectracraft_mono::platform;
use std::path::PathBuf;

enum LaunchMode {
    PrintPlatformInfo,
    RunGui {
        initial_wav: Option<PathBuf>,
        load_reference: bool,
    },
}

fn parse_launch_args<I>(args: I) -> LaunchMode
where
    I: IntoIterator<Item = String>,
{
    let mut initial_wav: Option<PathBuf> = None;
    let mut load_reference = false;
    let mut iter = args.into_iter().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--platform-info" | "--self-check" => return LaunchMode::PrintPlatformInfo,
            "--reference" | "--demo" => load_reference = true,
            "--open" => {
                if let Some(p) = iter.next() {
                    initial_wav = Some(platform::normalize_user_path(&p));
                }
            }
            other if !other.starts_with('-') && initial_wav.is_none() => {
                initial_wav = Some(platform::normalize_user_path(other));
            }
            _ => {}
        }
    }
    LaunchMode::RunGui {
        initial_wav,
        load_reference,
    }
}

fn main() {
    match parse_launch_args(std::env::args()) {
        LaunchMode::PrintPlatformInfo => {
            println!("{}", platform::platform_report_json());
        }
        LaunchMode::RunGui {
            initial_wav,
            load_reference,
        } => {
            application().run(move |cx: &mut App| {
                register_keybindings(cx);

                let bounds = Bounds::centered(None, size(px(1320.0), px(860.0)), cx);
                let initial_wav_for_window = initial_wav.clone();

                cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        titlebar: Some(TitlebarOptions {
                            title: Some("SpectraCraft — Native GPUI Workstation".into()),
                            ..Default::default()
                        }),
                        focus: true,
                        ..Default::default()
                    },
                    move |_window, cx| {
                        cx.new(|cx| {
                            let mut app = SpectraCraftGpuiApp::new(cx);
                            if let Some(wav_path) = initial_wav_for_window {
                                app.open_wav_path(wav_path, cx);
                            } else if load_reference {
                                app.load_builtin_reference(cx);
                            }
                            app
                        })
                    },
                )
                .expect("failed to open SpectraCraft GPUI workstation window");

                cx.on_window_closed(|cx, _window_id| {
                    cx.quit();
                })
                .detach();
                cx.activate(true);
            });
        }
    }
}
