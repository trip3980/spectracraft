//! Background worker & bounded spectrogram tile cache bridge for the GPUI frontend.
//!
//! Wraps `spectracraft_mono::app_jobs::JobRunner` and `SpectrogramTileCache` so all DSP,
//! WAV decoding, hashing, and spectrogram tile FFTs execute strictly off the UI thread.
//! Supports an explicit wakeup counter/condvar so GPUI async tasks wake on completions
//! without busy-polling timers and shut down cleanly via weak entity handles.

use spectracraft_mono::app_jobs::{CancellationToken, JobError, JobOutcome, JobRequest, JobRunner};
use spectracraft_mono::app_session::{
    ArtifactId, AudioArtifact, DryJobSpec, PostDryJobSpec, RegionEditJobSpec,
};
use spectracraft_mono::visualization::{
    FrequencyScale, SpectrogramTile, SpectrogramTileCache, SpectrogramTileKey, Visual,
};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// Default GPUI tile cache budget: 64 tiles / 32 MiB.
pub const MAX_CACHED_TILES: usize = 64;
pub const MAX_CACHED_TILE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Default)]
struct WakeState {
    seq: u64,
    closed: bool,
    wakers: Vec<Waker>,
}

/// Non-blocking async wakeup signal fired by worker threads when a `JobOutcome` is ready.
/// Uses `std::task::Waker` so neither GPUI's live executor nor `TestBackgroundExecutor` blocks.
#[derive(Default)]
pub struct WorkerWakeup {
    state: Mutex<WakeState>,
}

impl WorkerWakeup {
    pub fn notify(&self) {
        let wakers = if let Ok(mut guard) = self.state.lock() {
            guard.seq = guard.seq.wrapping_add(1);
            std::mem::take(&mut guard.wakers)
        } else {
            Vec::new()
        };
        for w in wakers {
            w.wake();
        }
    }

    pub fn close(&self) {
        let wakers = if let Ok(mut guard) = self.state.lock() {
            guard.closed = true;
            std::mem::take(&mut guard.wakers)
        } else {
            Vec::new()
        };
        for w in wakers {
            w.wake();
        }
    }

    /// Asynchronously wait until `seq` advances beyond `last_seq` or the bridge closes.
    pub fn wait_next_async(self: &Arc<Self>, last_seq: u64) -> WaitNextFuture {
        WaitNextFuture {
            wakeup: Arc::clone(self),
            last_seq,
        }
    }
}

pub struct WaitNextFuture {
    wakeup: Arc<WorkerWakeup>,
    last_seq: u64,
}

impl Future for WaitNextFuture {
    type Output = Option<u64>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Ok(mut guard) = self.wakeup.state.lock() else {
            return Poll::Ready(None);
        };
        if guard.closed {
            return Poll::Ready(None);
        }
        if guard.seq != self.last_seq {
            return Poll::Ready(Some(guard.seq));
        }
        guard.wakers.push(cx.waker().clone());
        Poll::Pending
    }
}

/// Viewport parameters describing a requested spectrogram tile.
#[derive(Clone, Copy, Debug)]
pub struct TileViewportRequest {
    pub frame_start: usize,
    pub frame_end: usize,
    pub width: u32,
    pub height: u32,
    pub max_hz: f32,
    pub scale: FrequencyScale,
}

/// Bridges `JobRunner`, bounded `SpectrogramTileCache`, and overview `Visual` cache for GPUI.
pub struct GpuiJobBridge {
    runner: JobRunner,
    wakeup: Arc<WorkerWakeup>,
    tile_cache: SpectrogramTileCache,
    in_flight_tiles: HashSet<SpectrogramTileKey>,
    visuals: HashMap<ArtifactId, Visual>,
    active_cancel: Option<CancellationToken>,
}

impl GpuiJobBridge {
    pub fn new() -> Self {
        let wakeup = Arc::new(WorkerWakeup::default());
        let wakeup_cb = wakeup.clone();
        let runner = JobRunner::new(
            2,
            16,
            Some(Arc::new(move || {
                wakeup_cb.notify();
            })),
        );
        Self {
            runner,
            wakeup,
            tile_cache: SpectrogramTileCache::new(MAX_CACHED_TILES, MAX_CACHED_TILE_BYTES),
            in_flight_tiles: HashSet::new(),
            visuals: HashMap::new(),
            active_cancel: None,
        }
    }

    pub fn wakeup(&self) -> Arc<WorkerWakeup> {
        self.wakeup.clone()
    }

    pub fn in_flight_count(&self) -> usize {
        self.runner.in_flight_count()
    }

    pub fn queued_count(&self) -> usize {
        self.runner.queued_count()
    }

    pub fn tile_cache(&self) -> &SpectrogramTileCache {
        &self.tile_cache
    }

    pub fn visual_for(&self, artifact_id: ArtifactId) -> Option<&Visual> {
        self.visuals.get(&artifact_id)
    }

    pub fn clear_visuals(&mut self) {
        self.visuals.clear();
        self.tile_cache.clear();
        self.in_flight_tiles.clear();
    }

    pub fn cancel_pending(&mut self) {
        if let Some(tok) = self.active_cancel.take() {
            tok.cancel();
        }
        self.runner.cancel_all_pending();
        self.in_flight_tiles.clear();
    }

    pub fn submit_load_wav(
        &mut self,
        path: PathBuf,
        artifact_id: ArtifactId,
    ) -> Result<(), JobError> {
        self.cancel_pending();
        let cancel = CancellationToken::new();
        self.active_cancel = Some(cancel.clone());
        self.runner.submit(JobRequest::LoadWav {
            path,
            artifact_id,
            cancel,
        })
    }

    pub fn submit_dry_regen(&mut self, spec: DryJobSpec) -> Result<(), JobError> {
        let cancel = CancellationToken::new();
        self.active_cancel = Some(cancel.clone());
        self.runner.submit(JobRequest::DryRegen { spec, cancel })
    }

    pub fn submit_region_edit(&mut self, spec: RegionEditJobSpec) -> Result<(), JobError> {
        let cancel = CancellationToken::new();
        self.active_cancel = Some(cancel.clone());
        self.runner.submit(JobRequest::RegionEdit { spec, cancel })
    }

    pub fn submit_post_dry(&mut self, spec: PostDryJobSpec) -> Result<(), JobError> {
        let cancel = CancellationToken::new();
        self.runner.submit(JobRequest::PostDry { spec, cancel })
    }

    pub fn submit_overview_visual(
        &mut self,
        artifact: Arc<AudioArtifact>,
        max_hz: f32,
    ) -> Result<(), JobError> {
        let cancel = CancellationToken::new();
        self.runner.submit(JobRequest::OverviewVisual {
            artifact,
            max_hz,
            cancel,
        })
    }

    /// Look up a spectrogram tile in the bounded LRU cache without blocking the UI thread.
    /// On cache miss, enqueues an asynchronous tile computation job once per key.
    pub fn get_or_request_tile(
        &mut self,
        artifact: &Arc<AudioArtifact>,
        req: TileViewportRequest,
    ) -> Option<Arc<SpectrogramTile>> {
        let key = SpectrogramTileKey {
            artifact_id: artifact.id,
            sample_rate: artifact.sample_rate,
            channel: 0,
            frame_start: req.frame_start,
            frame_end: req.frame_end,
            fft_size: SpectrogramTileKey::recommended_fft_size(artifact.sample_rate),
            width: req.width.clamp(64, 512),
            height: req.height.clamp(48, 192),
            max_hz_milli: (req.max_hz.max(500.0) * 1000.0).round() as u32,
            scale: req.scale,
        };
        if let Some(hit) = self.tile_cache.get(&key) {
            return Some(hit);
        }
        if !self.in_flight_tiles.contains(&key) {
            self.in_flight_tiles.insert(key);
            let _ = self.runner.submit(JobRequest::SpectrogramTile {
                artifact: artifact.clone(),
                key,
                cancel: CancellationToken::new(),
            });
        }
        None
    }

    /// Drain completed outcomes, caching any visualization or spectrogram tiles internally
    /// and returning session-level completions for publication through `AppSession`.
    pub fn drain_outcomes(&mut self) -> Vec<JobOutcome> {
        let raw = self.runner.drain_outcomes();
        let mut session_outcomes = Vec::new();
        for outcome in raw {
            match outcome {
                JobOutcome::OverviewVisualReady {
                    artifact_id,
                    visual,
                    ..
                } => {
                    self.visuals.insert(artifact_id, visual);
                }
                JobOutcome::SpectrogramTileReady { tile } => {
                    self.in_flight_tiles.remove(&tile.key);
                    self.tile_cache.insert(tile);
                }
                other => session_outcomes.push(other),
            }
        }
        session_outcomes
    }

    pub fn shutdown(&mut self) {
        self.cancel_pending();
        self.wakeup.close();
        self.runner.shutdown();
    }
}

impl Default for GpuiJobBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for GpuiJobBridge {
    fn drop(&mut self) {
        self.shutdown();
    }
}
