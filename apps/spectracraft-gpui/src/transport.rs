//! Playback transport controller for the experimental GPUI frontend.
//!
//! Owns the authoritative playback clock via `spectracraft_mono::platform::AudioPlayback`
//! (`rodio` / `cpal`) rather than advancing the playhead with UI timers.
//! Handles unavailable OS audio output devices honestly without panicking so headless
//! environments and CI runs remain deterministic.

use spectracraft_mono::app_session::AudioArtifact;
use spectracraft_mono::platform::AudioPlayback;
use std::sync::Arc;

/// Authoritative transport state for the GPUI workstation.
#[derive(Default)]
pub struct TransportController {
    active: Option<AudioPlayback>,
    cursor_secs: f64,
}

impl TransportController {
    pub fn is_playing(&self) -> bool {
        self.active
            .as_ref()
            .map(|p| !p.is_finished())
            .unwrap_or(false)
    }

    /// Authoritative playhead position in seconds (derived from audio device clock while playing).
    pub fn position_secs(&mut self, duration_secs: f64) -> f64 {
        if let Some(play) = &self.active {
            if play.is_finished() {
                if let Some(done) = self.active.take() {
                    let final_pos = done.stop(duration_secs.max(0.0));
                    self.cursor_secs = final_pos.clamp(0.0, duration_secs.max(0.0));
                }
            } else {
                self.cursor_secs = play.position_secs().clamp(0.0, duration_secs.max(0.0));
            }
        }
        self.cursor_secs
    }

    /// Set cursor position explicitly (stops active stream if playing; restarts if desired by caller).
    pub fn seek(&mut self, secs: f64, duration_secs: f64) {
        let clamped = secs.clamp(0.0, duration_secs.max(0.0));
        if let Some(play) = self.active.take() {
            let _ = play.stop(clamped);
        }
        self.cursor_secs = clamped;
    }

    /// Stop active audio playback and retain the authoritative stop position.
    pub fn stop(&mut self) -> f64 {
        if let Some(play) = self.active.take() {
            let pos = play.stop(self.cursor_secs);
            self.cursor_secs = pos;
        }
        self.cursor_secs
    }

    /// Start playback of `artifact` from the current cursor position (or `range` if provided).
    /// Returns `Ok(())` when the native OS audio stream started, or an honest error message when
    /// no audio output device is available.
    pub fn play_artifact(
        &mut self,
        artifact: &Arc<AudioArtifact>,
        selection: Option<[f64; 2]>,
    ) -> Result<(), String> {
        self.stop();
        let rate = artifact.sample_rate;
        let duration = artifact.duration_s();
        let (start_s, end_opt) = match selection {
            Some([a, b]) if b > a + 0.01 => (a.clamp(0.0, duration), Some(b.clamp(a, duration))),
            _ => {
                let s = if self.cursor_secs >= duration - 0.02 {
                    0.0
                } else {
                    self.cursor_secs.clamp(0.0, duration)
                };
                (s, None)
            }
        };
        let start_sample = (start_s * f64::from(rate)).round() as usize;
        let end_sample = end_opt.map(|e| (e * f64::from(rate)).round() as usize);

        match AudioPlayback::play_mono_slice(
            rate,
            &artifact.samples,
            start_sample,
            end_sample,
            start_s,
        ) {
            Ok(playback) => {
                self.cursor_secs = start_s;
                self.active = Some(playback);
                Ok(())
            }
            Err(err) => Err(format!("Audio output unavailable ({err})")),
        }
    }

    /// Toggle play/pause on the currently displayed artifact.
    pub fn toggle(
        &mut self,
        artifact: Option<&Arc<AudioArtifact>>,
        selection: Option<[f64; 2]>,
    ) -> Result<bool, String> {
        if self.is_playing() {
            self.stop();
            return Ok(false);
        }
        let Some(art) = artifact else {
            return Err("No audio artifact loaded for the selected take".to_owned());
        };
        self.play_artifact(art, selection)?;
        Ok(true)
    }
}

impl Drop for TransportController {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
