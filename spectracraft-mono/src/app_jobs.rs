//! Bounded, cancellable background job runner shared across SpectraCraft native frontends.
//!
//! Keeps all heavy DSP, FFT, WAV decoding, hashing, and visualization work off UI callbacks
//! and audio render threads. All worker errors are owned `Send + Sync` values (`JobError`),
//! in-flight work is bounded by explicit thread and queue limits, preview jobs coalesce on
//! superseding requests, and every worker thread is cleanly joined on shutdown / drop.

use crate::app_session::{
    load_validated_wav_artifact, ArtifactId, AudioArtifact, CompletionGuard, DryJobSpec,
    PostDryJobSpec, RegionEditJobSpec,
};
use crate::pipeline::{self, PipelineRenderOutput};
use crate::region_edit::{self, RegionEditOutput};
use crate::reperform::{self, Performance};
use crate::visualization::{self, SpectrogramTile, SpectrogramTileKey, Visual};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

/// Owned, `Send + Sync` error returned from background SpectraCraft jobs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobError {
    pub stage: &'static str,
    pub message: String,
    pub cancelled: bool,
}

impl std::fmt::Display for JobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.cancelled {
            write!(f, "{} cancelled: {}", self.stage, self.message)
        } else {
            write!(f, "{} failed: {}", self.stage, self.message)
        }
    }
}

impl std::error::Error for JobError {}

/// Cooperative cancellation token passed into background jobs.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Coalescing key so rapidly repeated slider/zoom previews supersede queued stale previews.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoalesceLane {
    None,
    PostDry,
    OverviewVisual,
}

/// Background job request submitted to `JobRunner`.
pub enum JobRequest {
    LoadWav {
        path: PathBuf,
        artifact_id: ArtifactId,
        cancel: CancellationToken,
    },
    DryRegen {
        spec: DryJobSpec,
        cancel: CancellationToken,
    },
    RegionEdit {
        spec: RegionEditJobSpec,
        cancel: CancellationToken,
    },
    PostDry {
        spec: PostDryJobSpec,
        cancel: CancellationToken,
    },
    OverviewVisual {
        artifact: Arc<AudioArtifact>,
        max_hz: f32,
        cancel: CancellationToken,
    },
    SpectrogramTile {
        artifact: Arc<AudioArtifact>,
        key: SpectrogramTileKey,
        cancel: CancellationToken,
    },
}

impl JobRequest {
    fn coalesce_lane(&self) -> CoalesceLane {
        match self {
            Self::PostDry { .. } => CoalesceLane::PostDry,
            Self::OverviewVisual { .. } => CoalesceLane::OverviewVisual,
            _ => CoalesceLane::None,
        }
    }

    fn cancel_token(&self) -> &CancellationToken {
        match self {
            Self::LoadWav { cancel, .. }
            | Self::DryRegen { cancel, .. }
            | Self::RegionEdit { cancel, .. }
            | Self::PostDry { cancel, .. }
            | Self::OverviewVisual { cancel, .. }
            | Self::SpectrogramTile { cancel, .. } => cancel,
        }
    }
}

/// Completed background job payload delivered back to the session owner.
pub enum JobOutcome {
    WavLoaded {
        path: PathBuf,
        name: String,
        artifact: AudioArtifact,
    },
    DryReady {
        guard: CompletionGuard,
        performance: Performance,
        samples: Vec<f32>,
    },
    RegionEditReady {
        guard: CompletionGuard,
        output: RegionEditOutput,
    },
    PostDryReady {
        guard: CompletionGuard,
        output: PipelineRenderOutput,
    },
    OverviewVisualReady {
        artifact_id: ArtifactId,
        max_hz: f32,
        visual: Visual,
    },
    SpectrogramTileReady {
        tile: Arc<SpectrogramTile>,
    },
    Failed {
        guard: Option<CompletionGuard>,
        error: JobError,
    },
}

struct QueueState {
    pending: VecDeque<JobRequest>,
    shutdown: bool,
    max_pending: usize,
    latest_post_dry_cancel: Option<CancellationToken>,
    latest_visual_cancel: Option<CancellationToken>,
}

/// Bounded multi-worker job runner with explicit shutdown ownership and preview coalescing.
pub struct JobRunner {
    shared: Arc<(Mutex<QueueState>, Condvar)>,
    rx: Mutex<mpsc::Receiver<JobOutcome>>,
    in_flight: Arc<AtomicUsize>,
    total_submitted: Arc<AtomicUsize>,
    total_coalesced: Arc<AtomicUsize>,
    workers: Vec<JoinHandle<()>>,
    worker_count: usize,
    max_pending: usize,
}

impl JobRunner {
    /// Spawn a bounded worker pool (`worker_count` clamped to `1..=4`, `max_pending` clamped to `2..=64`).
    /// `on_outcome` is invoked from the worker thread after pushing each completion so a GUI
    /// event loop (GPUI async channel or egui `request_repaint`) wakes without busy polling.
    pub fn new(
        worker_count: usize,
        max_pending: usize,
        on_outcome: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        let worker_count = worker_count.clamp(1, 4);
        let max_pending = max_pending.clamp(2, 64);
        let (tx, rx) = mpsc::channel::<JobOutcome>();
        let shared = Arc::new((
            Mutex::new(QueueState {
                pending: VecDeque::with_capacity(max_pending),
                shutdown: false,
                max_pending,
                latest_post_dry_cancel: None,
                latest_visual_cancel: None,
            }),
            Condvar::new(),
        ));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::with_capacity(worker_count);

        for idx in 0..worker_count {
            let shared_w = shared.clone();
            let tx_w = tx.clone();
            let in_flight_w = in_flight.clone();
            let wake_w = on_outcome.clone();
            let handle = thread::Builder::new()
                .name(format!("spectracraft-worker-{idx}"))
                .spawn(move || {
                    worker_loop(shared_w, tx_w, in_flight_w, wake_w);
                })
                .expect("failed to spawn spectracraft worker thread");
            workers.push(handle);
        }

        Self {
            shared,
            rx: Mutex::new(rx),
            in_flight,
            total_submitted: Arc::new(AtomicUsize::new(0)),
            total_coalesced: Arc::new(AtomicUsize::new(0)),
            workers,
            worker_count,
            max_pending,
        }
    }

    pub fn worker_count(&self) -> usize {
        self.worker_count
    }

    pub fn max_pending(&self) -> usize {
        self.max_pending
    }

    pub fn in_flight_count(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    pub fn queued_count(&self) -> usize {
        self.shared.0.lock().map(|q| q.pending.len()).unwrap_or(0)
    }

    pub fn total_submitted(&self) -> usize {
        self.total_submitted.load(Ordering::SeqCst)
    }

    pub fn total_coalesced(&self) -> usize {
        self.total_coalesced.load(Ordering::SeqCst)
    }

    /// Submit a job to the bounded queue. Preview jobs (`PostDry`, `OverviewVisual`) coalesce by
    /// cancelling and dropping any older queued job in the same lane.
    pub fn submit(&self, req: JobRequest) -> Result<(), JobError> {
        let (lock, cvar) = &*self.shared;
        let mut guard = lock.lock().map_err(|_| JobError {
            stage: "runner",
            message: "Worker queue lock poisoned".to_owned(),
            cancelled: false,
        })?;
        if guard.shutdown {
            return Err(JobError {
                stage: "runner",
                message: "JobRunner is shutting down".to_owned(),
                cancelled: true,
            });
        }

        let lane = req.coalesce_lane();
        if lane != CoalesceLane::None {
            if lane == CoalesceLane::PostDry {
                if let Some(prev) = guard
                    .latest_post_dry_cancel
                    .replace(req.cancel_token().clone())
                {
                    prev.cancel();
                }
            } else if lane == CoalesceLane::OverviewVisual {
                if let Some(prev) = guard
                    .latest_visual_cancel
                    .replace(req.cancel_token().clone())
                {
                    prev.cancel();
                }
            }
            let before = guard.pending.len();
            guard.pending.retain(|queued| {
                if queued.coalesce_lane() == lane {
                    queued.cancel_token().cancel();
                    false
                } else {
                    true
                }
            });
            let removed = before.saturating_sub(guard.pending.len());
            if removed > 0 {
                self.total_coalesced.fetch_add(removed, Ordering::SeqCst);
            }
        }

        if guard.pending.len() >= guard.max_pending {
            // Drop oldest coalescable tile/visual preview if full; otherwise reject cleanly.
            if let Some(pos) = guard.pending.iter().position(|r| {
                matches!(
                    r,
                    JobRequest::SpectrogramTile { .. } | JobRequest::OverviewVisual { .. }
                )
            }) {
                if let Some(dropped) = guard.pending.remove(pos) {
                    dropped.cancel_token().cancel();
                    self.total_coalesced.fetch_add(1, Ordering::SeqCst);
                }
            } else {
                return Err(JobError {
                    stage: "runner",
                    message: format!("Bounded job queue full ({} pending)", guard.max_pending),
                    cancelled: false,
                });
            }
        }

        guard.pending.push_back(req);
        self.total_submitted.fetch_add(1, Ordering::SeqCst);
        cvar.notify_one();
        Ok(())
    }

    /// Cancel all queued and tracked preview jobs immediately (e.g., on source swap or window close).
    pub fn cancel_all_pending(&self) {
        let (lock, _) = &*self.shared;
        if let Ok(mut guard) = lock.lock() {
            if let Some(tok) = guard.latest_post_dry_cancel.take() {
                tok.cancel();
            }
            if let Some(tok) = guard.latest_visual_cancel.take() {
                tok.cancel();
            }
            while let Some(job) = guard.pending.pop_front() {
                job.cancel_token().cancel();
            }
        }
    }

    /// Non-blocking drain of all ready job outcomes.
    pub fn drain_outcomes(&self) -> Vec<JobOutcome> {
        let mut out = Vec::new();
        if let Ok(rx) = self.rx.lock() {
            while let Ok(item) = rx.try_recv() {
                out.push(item);
            }
        }
        out
    }

    /// Blocking receive with timeout (useful for deterministic headless integration tests).
    pub fn recv_timeout(&self, timeout: std::time::Duration) -> Option<JobOutcome> {
        self.rx.lock().ok()?.recv_timeout(timeout).ok()
    }

    /// Cancel pending work, wake all workers, and join every worker thread cleanly.
    pub fn shutdown(&mut self) {
        {
            let (lock, cvar) = &*self.shared;
            if let Ok(mut guard) = lock.lock() {
                guard.shutdown = true;
                if let Some(tok) = guard.latest_post_dry_cancel.take() {
                    tok.cancel();
                }
                if let Some(tok) = guard.latest_visual_cancel.take() {
                    tok.cancel();
                }
                while let Some(job) = guard.pending.pop_front() {
                    job.cancel_token().cancel();
                }
            }
            cvar.notify_all();
        }
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}

impl Drop for JobRunner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker_loop(
    shared: Arc<(Mutex<QueueState>, Condvar)>,
    tx: mpsc::Sender<JobOutcome>,
    in_flight: Arc<AtomicUsize>,
    on_outcome: Option<Arc<dyn Fn() + Send + Sync>>,
) {
    let (lock, cvar) = &*shared;
    loop {
        let req = {
            let mut guard = match lock.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            while guard.pending.is_empty() && !guard.shutdown {
                guard = match cvar.wait(guard) {
                    Ok(g) => g,
                    Err(_) => return,
                };
            }
            if guard.shutdown && guard.pending.is_empty() {
                return;
            }
            match guard.pending.pop_front() {
                Some(job) => job,
                None => continue,
            }
        };

        in_flight.fetch_add(1, Ordering::SeqCst);
        let outcome = execute_job(req);
        in_flight.fetch_sub(1, Ordering::SeqCst);

        if let Some(outcome) = outcome {
            let _ = tx.send(outcome);
            if let Some(wake) = &on_outcome {
                wake();
            }
        }
    }
}

/// Execute a single job synchronously (also callable directly in deterministic unit tests).
pub fn execute_job(req: JobRequest) -> Option<JobOutcome> {
    match req {
        JobRequest::LoadWav {
            path,
            artifact_id,
            cancel,
        } => {
            if cancel.is_cancelled() {
                return Some(JobOutcome::Failed {
                    guard: None,
                    error: JobError {
                        stage: "load_wav",
                        message: "Cancelled before decode".to_owned(),
                        cancelled: true,
                    },
                });
            }
            match load_validated_wav_artifact(&path, artifact_id) {
                Ok((artifact, name)) => {
                    if cancel.is_cancelled() {
                        return None;
                    }
                    Some(JobOutcome::WavLoaded {
                        path,
                        name,
                        artifact,
                    })
                }
                Err(message) => Some(JobOutcome::Failed {
                    guard: None,
                    error: JobError {
                        stage: "load_wav",
                        message,
                        cancelled: false,
                    },
                }),
            }
        }
        JobRequest::DryRegen { spec, cancel } => {
            if cancel.is_cancelled() {
                return Some(JobOutcome::Failed {
                    guard: Some(spec.guard),
                    error: JobError {
                        stage: "dry_regen",
                        message: "Cancelled before Stage 1 synthesis".to_owned(),
                        cancelled: true,
                    },
                });
            }
            let [s0, s1] = spec.excerpt_span;
            let end = s1.min(spec.source_samples.len());
            let start = s0.min(end);
            let excerpt = &spec.source_samples[start..end];
            let perf = match reperform::analyze_guitar(excerpt, spec.sample_rate) {
                Ok(p) => p,
                Err(e) => {
                    return Some(JobOutcome::Failed {
                        guard: Some(spec.guard),
                        error: JobError {
                            stage: "dry_regen",
                            message: format!("Stage 1 cue analysis failed: {e}"),
                            cancelled: false,
                        },
                    });
                }
            };
            if cancel.is_cancelled() {
                return Some(JobOutcome::Failed {
                    guard: Some(spec.guard),
                    error: JobError {
                        stage: "dry_regen",
                        message: "Cancelled after cue analysis".to_owned(),
                        cancelled: true,
                    },
                });
            }
            match perf.synthesize_dry_for_source_with_voicing(&spec.source_entry, &spec.voicing) {
                Ok(samples) => {
                    if cancel.is_cancelled() {
                        return Some(JobOutcome::Failed {
                            guard: Some(spec.guard),
                            error: JobError {
                                stage: "dry_regen",
                                message: "Cancelled after synthesis".to_owned(),
                                cancelled: true,
                            },
                        });
                    }
                    Some(JobOutcome::DryReady {
                        guard: spec.guard,
                        performance: perf,
                        samples,
                    })
                }
                Err(e) => Some(JobOutcome::Failed {
                    guard: Some(spec.guard),
                    error: JobError {
                        stage: "dry_regen",
                        message: format!("Stage 1 synthesis failed: {e}"),
                        cancelled: false,
                    },
                }),
            }
        }
        JobRequest::RegionEdit { spec, cancel } => {
            if cancel.is_cancelled() {
                return Some(JobOutcome::Failed {
                    guard: Some(spec.guard),
                    error: JobError {
                        stage: "region_edit",
                        message: "Cancelled before Stage 2 edit".to_owned(),
                        cancelled: true,
                    },
                });
            }
            match region_edit::apply_dry_region_edit(
                &spec.dry_samples,
                &spec.performance,
                &spec.source_entry,
                &spec.voicing,
                &spec.edit,
            ) {
                Ok(output) => Some(JobOutcome::RegionEditReady {
                    guard: spec.guard,
                    output,
                }),
                Err(e) => Some(JobOutcome::Failed {
                    guard: Some(spec.guard),
                    error: JobError {
                        stage: "region_edit",
                        message: format!("Stage 2 region edit failed: {e}"),
                        cancelled: false,
                    },
                }),
            }
        }
        JobRequest::PostDry { spec, cancel } => {
            if cancel.is_cancelled() {
                return None;
            }
            let output = pipeline::run_post_dry_pipeline(
                &spec.dry_samples,
                spec.sample_rate,
                &spec.config,
                &spec.sources,
                &spec.mics,
                &spec.surfaces,
            );
            if cancel.is_cancelled() {
                return None;
            }
            Some(JobOutcome::PostDryReady {
                guard: spec.guard,
                output,
            })
        }
        JobRequest::OverviewVisual {
            artifact,
            max_hz,
            cancel,
        } => {
            if cancel.is_cancelled() {
                return None;
            }
            let visual = Visual::measure_range(&artifact.samples, artifact.sample_rate, max_hz);
            if cancel.is_cancelled() {
                return None;
            }
            Some(JobOutcome::OverviewVisualReady {
                artifact_id: artifact.id,
                max_hz,
                visual,
            })
        }
        JobRequest::SpectrogramTile {
            artifact,
            key,
            cancel,
        } => {
            if cancel.is_cancelled() {
                return None;
            }
            let tile = Arc::new(visualization::compute_spectrogram_tile(
                &artifact.samples,
                key,
            ));
            if cancel.is_cancelled() {
                return None;
            }
            Some(JobOutcome::SpectrogramTileReady { tile })
        }
    }
}
