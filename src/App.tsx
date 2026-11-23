import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Box,
  BoxSelect,
  CheckCircle2,
  Crosshair,
  Download,
  Eye,
  FolderOpen,
  Layers,
  MousePointer2,
  Music2,
  Pause,
  Play,
  Redo2,
  RefreshCw,
  Scissors,
  Search,
  SkipBack,
  Undo2,
  Wrench,
  ZoomIn,
  ZoomOut,
} from 'lucide-react';
import {
  AudioViewport,
  type CanvasTool,
  type CursorReadout,
  type DisplayMode,
} from './components/AudioViewport.tsx';
import { ExportModal } from './components/ExportModal.tsx';
import { FloatingPanelWindow } from './components/FloatingPanelWindow.tsx';
import { ResynthesisLineageModal } from './components/ResynthesisLineageModal.tsx';
import { StudioBottomDock, type BottomDockTab } from './components/StudioBottomDock.tsx';
import { StudioInspector, type InspectorTab } from './components/StudioInspector.tsx';
import { StudioPlanCanvas } from './components/StudioPlanCanvas.tsx';
import { buildDifferenceBuffer } from './lib/audioCanvas.ts';
import {
  countPassedDryCriteria,
  emptyDryListeningChecklist,
  isDryListeningPassed,
  type DryCriterionKey,
  type DryListeningChecklist,
} from './lib/contract.ts';
import {
  addDryRegion,
  createInitialDryRegionHistory,
  deleteDryRegion,
  redoDryRegionHistory,
  selectDryRegion,
  serializeDryRegionForRust,
  undoDryRegionHistory,
  updateDryRegion,
  type DryRegion,
  type DryRegionKind,
} from './lib/dryRegions.ts';
import {
  DEFAULT_PIPELINE_STAGES,
  DEFAULT_VOICING_CONTROLS,
  computeAcousticFingerprint,
  createHistory,
  makeRoomPreset,
  pushHistory,
  redoHistory,
  serializePluginSlotForRust,
  serializeVoicingForRust,
  setMicWorldPosM,
  setSourcePosM,
  toRustCaptureConfig,
  undoHistory,
  type DryRegionEditResponse,
  type PipelineRenderResponse,
  type PipelineStagesState,
  type RoomPresetKey,
  type RustAcousticAnalysis,
  type RustCaptureResult,
  type RustSourceRegenResult,
  type RustStudioCatalogs,
  type StudioRigState,
  type Vec3,
  type VoicingControlsState,
} from './lib/studioRig.ts';
import {
  buildStructuredSynthesisPrompt,
  type ReferenceShape,
} from './lib/synthesisPrompt.ts';
import { useAudioEngine, type TakeMode } from './lib/useAudioEngine.ts';
import {
  ALL_VARIANTS,
  buildVariantSelection,
  computeEffectiveLayerGains,
  type GapRepairResult,
  type LayerChannel,
  type LayerSessionResult,
  type NoteEditPatch,
  type SessionPayload,
  type VariantKey,
  type VariantsManifest,
} from './lib/workbench.ts';
import {
  createDefaultWorkspaceLayout,
  loadPersistedWorkspaceLayout,
  movePaneInOrder,
  resizeBottomDockHeight,
  resizeInspectorWidth,
  resizeSpatialWidth,
  savePersistedWorkspaceLayout,
  setPaneFloating,
  updateFloatingBounds,
  type WorkspaceLayoutState,
  type WorkspacePaneId,
} from './lib/workspaceLayout.ts';

type MenuId = 'file' | 'edit' | 'audio' | 'view' | 'room' | null;

const INITIAL_LAYER_CHANNELS: LayerChannel[] = [
  { id: 'pitch', label: 'Pitch', description: 'Fundamental & harmonic ridges', url: '', gain: 1, muted: false, solo: false },
  { id: 'ringing', label: 'Faint ringing', description: 'Resonant sustain & body modes', url: '', gain: 1, muted: false, solo: false },
  { id: 'texture', label: 'Texture', description: 'Modeled broadband texture', url: '', gain: 1, muted: false, solo: false },
  { id: 'breath_accent', label: 'Breath accent', description: 'Transient & breath accent cues', url: '', gain: 1, muted: false, solo: false },
];

export default function App() {
  // Core session state — starts EMPTY until user explicitly opens a WAV or Reference
  const [session, setSession] = useState<SessionPayload | null>(null);
  const [statusText, setStatusText] = useState<string>('Ready');
  const [statusTone, setStatusTone] = useState<'idle' | 'busy' | 'ready' | 'error'>('idle');
  const [loadingAudio, setLoadingAudio] = useState(false);

  // Menus & persisted ArtCraft workspace layout (purely visual chrome — never touches rig or audio)
  const [openMenu, setOpenMenu] = useState<MenuId>(null);
  const [activeTool, setActiveTool] = useState<CanvasTool>('scrub');
  const [layout, setLayout] = useState<WorkspaceLayoutState>(() => loadPersistedWorkspaceLayout());
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>('source');
  const [dockTab, setDockTab] = useState<BottomDockTab>('review');
  const [exportModalOpen, setExportModalOpen] = useState(false);
  const [lineageModalOpen, setLineageModalOpen] = useState(false);
  const [isDraggingFile, setIsDraggingFile] = useState(false);

  useEffect(() => {
    savePersistedWorkspaceLayout(layout);
  }, [layout]);

  const setDockCollapsed = useCallback((updater: boolean | ((prev: boolean) => boolean)) => {
    setLayout((prev) => ({
      ...prev,
      bottomDockCollapsed:
        typeof updater === 'function' ? updater(prev.bottomDockCollapsed) : updater,
    }));
  }, []);

  const setInspectorOpen = useCallback((updater: boolean | ((prev: boolean) => boolean)) => {
    setLayout((prev) => ({
      ...prev,
      inspectorOpen: typeof updater === 'function' ? updater(prev.inspectorOpen) : updater,
    }));
  }, []);

  const inspectorOpen = layout.inspectorOpen;
  const dockCollapsed = layout.bottomDockCollapsed;

  // Real Rust Studio Catalogs (138 sources, 74 factual mics, 32 surfaces, 5 room presets)
  const [catalogs, setCatalogs] = useState<RustStudioCatalogs | null>(null);

  // Viewport & evidence-inspection settings (visual only — never alter output audio)
  const [displayMode, setDisplayMode] = useState<DisplayMode>('both');
  const [displayMaxHz, setDisplayMaxHz] = useState<number>(12000);
  const [spectrogramDbFloor, setSpectrogramDbFloor] = useState<number>(-96);
  const [showEvidenceOverlay, setShowEvidenceOverlay] = useState<boolean>(false);
  const [previewBitDepth, setPreviewBitDepth] = useState<32 | 24 | 16>(32);
  const [levelMatchAb, setLevelMatchAb] = useState<boolean>(true);
  const [viewStartS, setViewStartS] = useState(0);
  const [viewEndS, setViewEndS] = useState(1);
  const [selection, setSelection] = useState<[number, number] | null>(null);
  const [cursor, setCursor] = useState<CursorReadout | null>(null);

  // Decoded real AudioBuffers
  const [origBuffer, setOrigBuffer] = useState<AudioBuffer | null>(null);
  const [regenBuffer, setRegenBuffer] = useState<AudioBuffer | null>(null);
  const [capturedBuffer, setCapturedBuffer] = useState<AudioBuffer | null>(null);
  const [diffBuffer, setDiffBuffer] = useState<AudioBuffer | null>(null);
  const [activeDryUrl, setActiveDryUrl] = useState<string | null>(null);
  const [activeCuesUrl, setActiveCuesUrl] = useState<string | null>(null);
  const [activeCuesRunId, setActiveCuesRunId] = useState<string | null>(null);
  const [referenceShape, setReferenceShape] = useState<ReferenceShape | null>(null);
  const shapeSeqRef = useRef(0);

  // Editable Layers (4 rows)
  const [layerChannels, setLayerChannels] = useState<LayerChannel[]>(INITIAL_LAYER_CHANNELS);
  const [layerSession, setLayerSession] = useState<LayerSessionResult | null>(null);
  const [layerBuffers, setLayerBuffers] = useState<Partial<Record<LayerChannel['id'], AudioBuffer>>>({});
  const [viewedLayerId, setViewedLayerId] = useState<LayerChannel['id'] | null>(null);
  const [includeTexture, setIncludeTexture] = useState(false);
  const [layersBusy, setLayersBusy] = useState(false);

  // Cues, Source Regeneration, Local Repair, Variants & 5-Criterion Human Listening Gate
  const [cuePatches, setCuePatches] = useState<Record<number, NoteEditPatch>>({});
  const [cuesBusy, setCuesBusy] = useState(false);
  const [sourceRegenBusy, setSourceRegenBusy] = useState(false);
  const [sourceRegenResult, setSourceRegenResult] = useState<RustSourceRegenResult | null>(null);
  const [repairResult, setRepairResult] = useState<GapRepairResult | null>(null);
  const [repairBusy, setRepairBusy] = useState(false);
  const [dryChecklist, setDryChecklist] = useState<DryListeningChecklist>(() =>
    emptyDryListeningChecklist(),
  );
  const [selectedVariants, setSelectedVariants] = useState<VariantKey[]>([...ALL_VARIANTS]);
  const [variants, setVariants] = useState<VariantsManifest | null>(null);
  const [variantsBusy, setVariantsBusy] = useState(false);

  // Stage 1 DDSP Voicing + Extensible Catalog Reload
  const [voicing, setVoicing] = useState<VoicingControlsState>(DEFAULT_VOICING_CONTROLS);
  const [reloadingCatalogs, setReloadingCatalogs] = useState(false);

  // Stage 2 Surgical Region Editing on Cached Dry Take (independent Undo/Redo + Solo Loop A/B)
  const [regionHistory, setRegionHistory] = useState(() => createInitialDryRegionHistory());
  const [regionEditBusy, setRegionEditBusy] = useState(false);
  const [regionEditResult, setRegionEditResult] = useState<DryRegionEditResponse | null>(null);
  const [regionBeforeBuf, setRegionBeforeBuf] = useState<AudioBuffer | null>(null);
  const [regionAfterBuf, setRegionAfterBuf] = useState<AudioBuffer | null>(null);
  const [regionPreviewAb, setRegionPreviewAb] = useState<'after' | 'before'>('after');
  const [regionPreviewPlaying, setRegionPreviewPlaying] = useState(false);
  const regionLoopSourceRef = useRef<AudioBufferSourceNode | null>(null);
  const regionEditSeqRef = useRef(0);

  // Stages 3, 4, 5 Independently Bypassable Post-Dry Pipeline (all OFF by default)
  const [pipelineStages, setPipelineStages] = useState<PipelineStagesState>(DEFAULT_PIPELINE_STAGES);
  const [pipelineBusy, setPipelineBusy] = useState(false);
  const [pipelineResult, setPipelineResult] = useState<PipelineRenderResponse | null>(null);

  // Studio Rig + Real Rust 6-Band Capture State + Undo/Redo History
  const [rigHistory, setRigHistory] = useState(() => createHistory<StudioRigState>(makeRoomPreset('small')));
  const rig = rigHistory.present;
  const [livePreview, setLivePreview] = useState(false);
  const [captureBusy, setCaptureBusy] = useState(false);
  const [captureResult, setCaptureResult] = useState<RustCaptureResult | null>(null);
  const [rustAcousticAnalysis, setRustAcousticAnalysis] = useState<RustAcousticAnalysis | null>(null);
  const captureSeqRef = useRef(0);
  const analysisSeqRef = useRef(0);

  const fileInputRef = useRef<HTMLInputElement | null>(null);

  const sampleRate = session?.verification.sample_rate ?? 48000;
  const durationS = session?.verification.duration_s ?? 0;
  const dryPassedCount = countPassedDryCriteria(dryChecklist);
  const dryReviewApproved = isDryListeningPassed(dryChecklist);

  // Dedicated Web Audio playback & crossfade hook (Refactoring §3d)
  const {
    takeMode,
    setTakeMode,
    takeModeRef,
    isPlaying,
    isPlayingRef,
    playheadS,
    playheadRef,
    ensureAudioContext,
    decodeWavUrl,
    crossfadeToBuffer: crossfadePlaybackToBuffer,
    togglePlay: handleTogglePlay,
    seek: handleSeek,
    switchTake: handleSwitchTake,
    resetTransport,
  } = useAudioEngine({
    origBuffer,
    regenBuffer,
    capturedBuffer,
    diffBuffer,
    previewBitDepth,
    levelMatchAb,
    abDryMatchGainDb: session?.verification.ab_dry_match_gain_db ?? 0,
    durationS,
    hasSession: Boolean(session),
  });

  // Fetch real Rust studio catalogs once on mount
  useEffect(() => {
    let active = true;
    fetch('/api/studio/catalogs')
      .then((res) => (res.ok ? res.json() : Promise.reject(new Error(`HTTP ${res.status}`))))
      .then((data: RustStudioCatalogs) => {
        if (active) setCatalogs(data);
      })
      .catch((err) => {
        if (active) {
          setStatusTone('error');
          setStatusText(
            `Rust catalog load failed: ${err instanceof Error ? err.message : String(err)}`,
          );
        }
      });
    return () => {
      active = false;
    };
  }, []);

  // Fetch authoritative Rust acoustic analysis (Sabine RT60, axial modes, 343 m/s delay, polar trig, 3:1 guidance)
  // whenever acoustic rig state changes (display-only changes like unit/orbit/overlays do NOT trigger fetches).
  const rigAcousticKey = useMemo(() => computeAcousticFingerprint(rig), [rig]);
  useEffect(() => {
    const seq = ++analysisSeqRef.current;
    const timer = window.setTimeout(() => {
      fetch('/api/studio/analyze', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ rigConfig: toRustCaptureConfig(rig) }),
      })
        .then((res) => (res.ok ? res.json() : null))
        .then((data: RustAcousticAnalysis | null) => {
          if (data && analysisSeqRef.current === seq) {
            setRustAcousticAnalysis(data);
          }
        })
        .catch(() => {
          // Local exact mirror remains active without blocking UI
        });
    }, 35);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rigAcousticKey]);

  // Fetch authoritative Rust ReferenceShape whenever session, selected source (for out-of-range check),
  // or edited cues run changes. Display-only actions never re-fetch or alter the shape.
  useEffect(() => {
    if (!session) {
      setReferenceShape(null);
      return;
    }
    const seq = ++shapeSeqRef.current;
    const qs = new URLSearchParams({ sourceIndex: String(rig.sourceIndex) });
    if (activeCuesRunId) qs.set('cuesRunId', activeCuesRunId);
    fetch(`/api/sessions/${encodeURIComponent(session.id)}/reference-shape?${qs.toString()}`)
      .then((res) => (res.ok ? res.json() : null))
      .then((data: ReferenceShape | null) => {
        if (data && shapeSeqRef.current === seq) {
          setReferenceShape(data);
        }
      })
      .catch(() => {
        // Keep previous shape on transient failure
      });
  }, [session, rig.sourceIndex, activeCuesRunId]);

  const structuredPrompt = useMemo(() => {
    if (!session || !referenceShape || !catalogs) return null;
    return buildStructuredSynthesisPrompt({
      sessionId: session.id,
      sourceFileName: session.label,
      shape: referenceShape,
      rig,
      sources: catalogs.sources,
      mics: catalogs.mics,
      surfaces: catalogs.surfaces,
      analysis:
        rustAcousticAnalysis ??
        pipelineResult?.acoustic_analysis ??
        captureResult?.acoustic_analysis ??
        null,
      voicing,
      pipelineStages,
    });
  }, [
    session,
    referenceShape,
    catalogs,
    rig,
    rustAcousticAnalysis,
    pipelineResult,
    captureResult,
    voicing,
    pipelineStages,
  ]);

  const hydrateSessionAudio = useCallback(
    async (payload: SessionPayload, dryUrlOverride?: string, cuesUrlOverride?: string) => {
      setLoadingAudio(true);
      try {
        const dryUrl = dryUrlOverride ?? payload.files.dryWav;
        const cuesUrl = cuesUrlOverride ?? payload.files.cuesJson;
        const [orig, regen] = await Promise.all([
          decodeWavUrl(payload.files.sourceWav),
          decodeWavUrl(dryUrl),
        ]);
        const ctx = ensureAudioContext();
        const diff = buildDifferenceBuffer(ctx, orig, regen);
        setOrigBuffer(orig);
        setRegenBuffer(regen);
        setDiffBuffer(diff);
        setActiveDryUrl(dryUrl);
        setActiveCuesUrl(cuesUrl);
        return { orig, regen, diff };
      } finally {
        setLoadingAudio(false);
      }
    },
    [decodeWavUrl, ensureAudioContext],
  );

  const loadSessionPayload = useCallback(
    async (payload: SessionPayload) => {
      resetTransport('regenerated');
      setSession(payload);
      setViewStartS(0);
      setViewEndS(payload.verification.duration_s);
      setSelection(null);
      setViewedLayerId(null);
      setLayerSession(null);
      setLayerBuffers({});
      setLayerChannels(INITIAL_LAYER_CHANNELS);
      setCuePatches({});
      setActiveCuesRunId(null);
      setSourceRegenResult(null);
      setRepairResult(null);
      setVariants(payload.variants);
      setCapturedBuffer(null);
      setCaptureResult(null);
      setPipelineResult(null);
      setRegionHistory(createInitialDryRegionHistory());
      setRegionEditResult(null);
      setRegionBeforeBuf(null);
      setRegionAfterBuf(null);
      setDryChecklist(emptyDryListeningChecklist());
      setLivePreview(false);
      await hydrateSessionAudio(payload);
      setStatusTone('ready');
      setStatusText(`Loaded ${payload.label} · Stage 1 dry take ready (post-dry stages OFF by default)`);
    },
    [hydrateSessionAudio, resetTransport],
  );

  const handleOpenReference = useCallback(async () => {
    setOpenMenu(null);
    setStatusTone('busy');
    setStatusText('Running Rust guitar-verify on 48 kHz Slow Solo reference…');
    try {
      const res = await fetch('/api/sessions/default');
      const data = await res.json();
      if (!res.ok) throw new Error(data.error || 'Failed to load reference session');
      await loadSessionPayload(data as SessionPayload);
    } catch (err) {
      setStatusTone('error');
      setStatusText(err instanceof Error ? err.message : String(err));
    }
  }, [loadSessionPayload]);

  const handleUploadWavFile = useCallback(
    async (file: File) => {
      setOpenMenu(null);
      setStatusTone('busy');
      setStatusText(`Analyzing ${file.name} in Rust engine…`);
      try {
        const buf = await file.arrayBuffer();
        const res = await fetch('/api/sessions/upload', {
          method: 'POST',
          headers: {
            'Content-Type': 'application/octet-stream',
            'X-Filename': file.name,
          },
          body: buf,
        });
        const data = await res.json();
        if (!res.ok) throw new Error(data.error || 'Rust analysis rejected this WAV');
        await loadSessionPayload(data as SessionPayload);
      } catch (err) {
        setStatusTone('error');
        setStatusText(err instanceof Error ? err.message : String(err));
      }
    },
    [loadSessionPayload],
  );

  const handleCloseFile = useCallback(() => {
    setOpenMenu(null);
    resetTransport('regenerated');
    setSession(null);
    setOrigBuffer(null);
    setRegenBuffer(null);
    setCapturedBuffer(null);
    setCaptureResult(null);
    setDiffBuffer(null);
    setActiveDryUrl(null);
    setActiveCuesUrl(null);
    setLayerSession(null);
    setLayerBuffers({});
    setViewedLayerId(null);
    setSelection(null);
    setSourceRegenResult(null);
    setDryChecklist(emptyDryListeningChecklist());
    setLivePreview(false);
    setStatusTone('idle');
    setStatusText('Ready');
  }, [resetTransport]);

  const handleUpdateRig = useCallback((next: StudioRigState, commit = true) => {
    setRigHistory((prev) =>
      commit ? pushHistory(prev, next) : { ...prev, present: next },
    );
  }, []);

  const handleUndo = useCallback(() => {
    setOpenMenu(null);
    setRigHistory((prev) => undoHistory(prev));
  }, []);

  const handleRedo = useCallback(() => {
    setOpenMenu(null);
    setRigHistory((prev) => redoHistory(prev));
  }, []);

  const handleAnalyzeLayers = useCallback(async () => {
    if (!session) return;
    setLayersBusy(true);
    setStatusTone('busy');
    setStatusText('Running Rust analyze-layers & render-layers…');
    try {
      const startS = selection ? selection[0] : Math.min(20, Math.max(0, durationS - 7));
      const endS = selection ? selection[1] : Math.min(durationS, startS + 7);
      const res = await fetch(`/api/sessions/${session.id}/layers`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ startS, endS, faintRidges: true }),
      });
      const data = (await res.json()) as LayerSessionResult & { error?: string };
      if (!res.ok) throw new Error(data.error || 'Layer analysis failed');
      setLayerSession(data);
      const decodedEntries = await Promise.all(
        data.stems.map(async (st) => [st.id, await decodeWavUrl(st.url)] as const),
      );
      const nextMap: Partial<Record<LayerChannel['id'], AudioBuffer>> = {};
      for (const [id, buf] of decodedEntries) nextMap[id] = buf;
      setLayerBuffers(nextMap);
      setLayerChannels((prev) =>
        prev.map((ch) => {
          const found = data.stems.find((s) => s.id === ch.id);
          return found ? { ...ch, url: found.url } : ch;
        }),
      );
      setStatusTone('ready');
      setStatusText(`Inspection layers ready (${data.startS.toFixed(2)}–${data.endS.toFixed(2)} s)`);
    } catch (err) {
      setStatusTone('error');
      setStatusText(err instanceof Error ? err.message : String(err));
    } finally {
      setLayersBusy(false);
    }
  }, [session, selection, durationS, decodeWavUrl]);

  const handleRenderEditedCues = useCallback(async () => {
    if (!session) return;
    setCuesBusy(true);
    setStatusTone('busy');
    setStatusText('Running source-free Rust render-dry-guitar…');
    try {
      const patches = Object.values(cuePatches);
      const res = await fetch(`/api/sessions/${session.id}/render-cues`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ patches }),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error || 'Cue render failed');
      setActiveCuesRunId(data.runId);
      // Contract invariant: new dry render resets human listening gate to 0/5 and invalidates old capture
      setDryChecklist(emptyDryListeningChecklist());
      setCapturedBuffer(null);
      setCaptureResult(null);
      setTakeMode((m) => (m === 'captured' ? 'regenerated' : m));
      setLivePreview(false);
      await hydrateSessionAudio(session, data.dryWavUrl, data.cuesJsonUrl);
      setStatusTone('ready');
      setStatusText(`New source-free dry WAV synthesized (run ${data.runId}) · Dry listening check reset to 0/5`);
    } catch (err) {
      setStatusTone('error');
      setStatusText(err instanceof Error ? err.message : String(err));
    } finally {
      setCuesBusy(false);
    }
  }, [session, cuePatches, hydrateSessionAudio]);

  const handleReloadCatalogs = useCallback(async () => {
    setReloadingCatalogs(true);
    try {
      const res = await fetch('/api/studio/reload-catalogs', { method: 'POST' });
      const data = (await res.json()) as RustStudioCatalogs & { error?: string };
      if (!res.ok) throw new Error(data.error || 'Catalog reload failed');
      setCatalogs(data);
      setStatusTone('ready');
      setStatusText(
        `Reloaded extensible catalog (${data.sources.length} sources, ${data.mics.length} mics, ${data.surfaces.length} surfaces)`,
      );
    } catch (err) {
      setStatusTone('error');
      setStatusText(err instanceof Error ? err.message : String(err));
    } finally {
      setReloadingCatalogs(false);
    }
  }, []);

  const handleRegenerateDryWithSource = useCallback(async () => {
    if (!session) return;
    setSourceRegenBusy(true);
    const chosen = catalogs?.sources[rig.sourceIndex];
    setStatusTone('busy');
    setStatusText(
      `Stage 1 · Regenerating dry take in Rust (${chosen ? chosen.name : `Source #${rig.sourceIndex}`} · ${voicing.characterTarget}/${voicing.straightenMode})…`,
    );
    try {
      const res = await fetch(`/api/sessions/${session.id}/regenerate-source`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          sourceIndex: rig.sourceIndex,
          cuesRunId: activeCuesRunId,
          voicing: serializeVoicingForRust(voicing),
        }),
      });
      const data = (await res.json()) as RustSourceRegenResult & { error?: string };
      if (!res.ok) throw new Error(data.error || 'Source-informed dry regeneration failed');
      setActiveCuesRunId(data.runId);
      setSourceRegenResult(data);
      // Fresh Stage 1 dry take invalidates cached downstream output until re-rendered
      setDryChecklist(emptyDryListeningChecklist());
      setCapturedBuffer(null);
      setCaptureResult(null);
      setPipelineResult(null);
      setRegionEditResult(null);
      setTakeMode((m) => (m === 'captured' ? 'regenerated' : m));
      const hydrated = await hydrateSessionAudio(session, data.dryWavUrl, data.cuesJsonUrl);
      if (isPlayingRef.current) {
        if (takeModeRef.current === 'regenerated' || takeModeRef.current === 'captured') {
          crossfadePlaybackToBuffer(hydrated.regen);
        } else if (takeModeRef.current === 'difference') {
          crossfadePlaybackToBuffer(hydrated.diff);
        }
      }
      setStatusTone('ready');
      setStatusText(
        `Stage 1 dry take cached (${data.sourceName} · centroid ${Math.round(data.spectral.spectral_centroid_hz)} Hz · SHA ${data.drySha256.slice(0, 10)})`,
      );
    } catch (err) {
      setStatusTone('error');
      setStatusText(err instanceof Error ? err.message : String(err));
    } finally {
      setSourceRegenBusy(false);
    }
  }, [
    session,
    catalogs,
    rig.sourceIndex,
    voicing,
    activeCuesRunId,
    hydrateSessionAudio,
    setTakeMode,
    isPlayingRef,
    takeModeRef,
    crossfadePlaybackToBuffer,
  ]);

  // Stage 2 Surgical Region Solo Loop Playback helpers
  const stopRegionLoop = useCallback(() => {
    if (regionLoopSourceRef.current) {
      try {
        regionLoopSourceRef.current.stop();
        regionLoopSourceRef.current.disconnect();
      } catch {
        // already stopped
      }
      regionLoopSourceRef.current = null;
    }
    setRegionPreviewPlaying(false);
  }, []);

  const startRegionLoopBuffer = useCallback(
    (buf: AudioBuffer | null) => {
      if (!buf) return;
      if (regionLoopSourceRef.current) {
        try {
          regionLoopSourceRef.current.stop();
          regionLoopSourceRef.current.disconnect();
        } catch {
          // ignore
        }
        regionLoopSourceRef.current = null;
      }
      const ctx = ensureAudioContext();
      if (ctx.state === 'suspended') {
        void ctx.resume();
      }
      const src = ctx.createBufferSource();
      src.buffer = buf;
      src.loop = true;
      src.connect(ctx.destination);
      src.start();
      regionLoopSourceRef.current = src;
      setRegionPreviewPlaying(true);
    },
    [ensureAudioContext],
  );

  useEffect(() => () => stopRegionLoop(), [stopRegionLoop]);

  const runDryRegionEdit = useCallback(
    async (region: DryRegion, commit: boolean) => {
      if (!session) return;
      const reqId = ++regionEditSeqRef.current;
      setRegionEditBusy(true);
      setStatusTone('busy');
      setStatusText(
        commit
          ? `Stage 2 · Applying ${region.action} fix to cached dry take (${region.startS.toFixed(2)}–${region.endS.toFixed(2)} s)…`
          : `Stage 2 · Previewing localized ${region.action} fix (${region.startS.toFixed(2)}–${region.endS.toFixed(2)} s)…`,
      );
      try {
        const res = await fetch(`/api/sessions/${session.id}/dry-region-edit`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            edit: serializeDryRegionForRust(region),
            sourceIndex: rig.sourceIndex,
            voicing: serializeVoicingForRust(voicing),
            cuesRunId: activeCuesRunId,
            commit,
          }),
        });
        const data = (await res.json()) as DryRegionEditResponse & { error?: string };
        if (!res.ok) throw new Error(data.error || 'Dry region edit failed');
        if (reqId !== regionEditSeqRef.current) return;

        const [beforeBuf, afterBuf] = await Promise.all([
          decodeWavUrl(data.beforeSliceUrl),
          decodeWavUrl(data.afterSliceUrl),
        ]);
        if (reqId !== regionEditSeqRef.current) return;

        setRegionEditResult(data);
        setRegionBeforeBuf(beforeBuf);
        setRegionAfterBuf(afterBuf);

        if (regionLoopSourceRef.current) {
          startRegionLoopBuffer(regionPreviewAb === 'before' ? beforeBuf : afterBuf);
        }

        if (commit) {
          setActiveCuesRunId(data.runId);
          setCapturedBuffer(null);
          setCaptureResult(null);
          setPipelineResult(null);
          setRegionHistory((prev) =>
            updateDryRegion(
              prev,
              region.id,
              {
                appliedCount: region.appliedCount + 1,
                lastAppliedSha: data.edited_sha256,
              },
              durationS,
              false,
            ),
          );
          const hydrated = await hydrateSessionAudio(session, data.editedWavUrl, activeCuesUrl ?? undefined);
          if (isPlayingRef.current && takeModeRef.current === 'regenerated') {
            crossfadePlaybackToBuffer(hydrated.regen);
          }
          setStatusTone('ready');
          setStatusText(
            `Stage 2 · Applied localized ${data.effective_action} (${data.affected_notes} note(s) · outside bit-identical: ${data.outside_bit_identical ? 'YES' : 'NO'} · SHA ${data.edited_sha256.slice(0, 10)})`,
          );
        } else {
          setStatusTone('ready');
          setStatusText(
            `Stage 2 · Region preview ready (${data.affected_notes} note(s) · outside bit-identical: ${data.outside_bit_identical ? 'YES' : 'NO'})`,
          );
        }
      } catch (err) {
        if (reqId !== regionEditSeqRef.current) return;
        setStatusTone('error');
        setStatusText(err instanceof Error ? err.message : String(err));
      } finally {
        if (reqId === regionEditSeqRef.current) {
          setRegionEditBusy(false);
        }
      }
    },
    [
      session,
      rig.sourceIndex,
      voicing,
      activeCuesRunId,
      activeCuesUrl,
      decodeWavUrl,
      durationS,
      hydrateSessionAudio,
      isPlayingRef,
      takeModeRef,
      crossfadePlaybackToBuffer,
      regionPreviewAb,
      startRegionLoopBuffer,
    ],
  );

  // Debounced live region preview update while dragging/resizing a region when solo loop is active
  const selectedRegionObj =
    regionHistory.regions.find((r) => r.id === regionHistory.selectedId) ?? null;
  const selectedRegionKey = selectedRegionObj
    ? `${selectedRegionObj.id}|${selectedRegionObj.startS}|${selectedRegionObj.endS}|${selectedRegionObj.minHz}|${selectedRegionObj.maxHz}|${selectedRegionObj.action}|${selectedRegionObj.strength}|${selectedRegionObj.crossfadeMs}`
    : '';
  useEffect(() => {
    if (!regionPreviewPlaying || !selectedRegionObj || !session) return;
    const timer = window.setTimeout(() => {
      void runDryRegionEdit(selectedRegionObj, false);
    }, 300);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedRegionKey, regionPreviewPlaying]);

  const handleToggleRegionPreviewPlay = useCallback(() => {
    if (regionPreviewPlaying) {
      stopRegionLoop();
    } else {
      const buf = regionPreviewAb === 'before' ? regionBeforeBuf : regionAfterBuf;
      startRegionLoopBuffer(buf);
    }
  }, [
    regionPreviewPlaying,
    stopRegionLoop,
    regionPreviewAb,
    regionBeforeBuf,
    regionAfterBuf,
    startRegionLoopBuffer,
  ]);

  const handleChangeRegionPreviewAb = useCallback(
    (mode: 'after' | 'before') => {
      setRegionPreviewAb(mode);
      if (regionPreviewPlaying) {
        startRegionLoopBuffer(mode === 'before' ? regionBeforeBuf : regionAfterBuf);
      }
    },
    [regionPreviewPlaying, regionBeforeBuf, regionAfterBuf, startRegionLoopBuffer],
  );

  const handleCreateDryRegion = useCallback(
    (input: {
      kind: DryRegionKind;
      startS: number;
      endS: number;
      minHz?: number | null;
      maxHz?: number | null;
    }) => {
      setRegionHistory((prev) => addDryRegion(prev, input, durationS));
      setDockCollapsed(false);
      setDockTab('regions');
    },
    [durationS, setDockCollapsed],
  );

  const handleQuickAddDryRegion = useCallback(
    (kind: DryRegionKind) => {
      const startS = selection ? selection[0] : Math.min(Math.max(0.1, durationS * 0.12), Math.max(0.1, durationS - 0.6));
      const endS = selection ? selection[1] : Math.min(durationS, startS + Math.min(1.0, Math.max(0.4, durationS * 0.25)));
      handleCreateDryRegion({
        kind,
        startS,
        endS,
        minHz: kind === 'marquee' ? 160 : null,
        maxHz: kind === 'marquee' ? 2400 : null,
      });
    },
    [selection, durationS, handleCreateDryRegion],
  );

  const handleApplyCharacterGuess = useCallback(
    (guess: 'natural' | 'analog' | 'digital', straightenRecommended: boolean) => {
      setVoicing((prev) => ({
        ...prev,
        characterTarget: guess,
        straightenMode: straightenRecommended ? 'light' : prev.straightenMode,
      }));
      setInspectorOpen(true);
      setInspectorTab('source');
      setStatusTone('ready');
      setStatusText(
        `Applied rough character guess (${guess}${straightenRecommended ? ' + light straighten' : ''}) to Stage 1 Dry Voicing`,
      );
    },
    [setInspectorOpen],
  );

  const handleRunGapRepair = useCallback(
    async (startS: number, gapMs: number) => {
      if (!session) return;
      setRepairBusy(true);
      setStatusTone('busy');
      setStatusText('Running Rust repair-gap…');
      try {
        const res = await fetch(`/api/sessions/${session.id}/repair-gap`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ startS, gapMs }),
        });
        const data = (await res.json()) as GapRepairResult & { error?: string };
        if (!res.ok) throw new Error(data.error || 'Gap repair failed');
        setRepairResult(data);
        setStatusTone('ready');
        setStatusText(`Local gap repair: ${data.status}`);
      } catch (err) {
        setStatusTone('error');
        setStatusText(err instanceof Error ? err.message : String(err));
      } finally {
        setRepairBusy(false);
      }
    },
    [session],
  );

  const handleRenderVariants = useCallback(async () => {
    if (!session || !dryReviewApproved) return;
    const sel = buildVariantSelection(selectedVariants);
    if (!sel) return;
    setVariantsBusy(true);
    setStatusTone('busy');
    setStatusText('Rendering post-processing variants in Rust…');
    try {
      const res = await fetch(`/api/sessions/${session.id}/variants`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          selection: sel,
          cuesRunId: activeCuesRunId,
          dryApproved: dryReviewApproved,
        }),
      });
      const data = (await res.json()) as VariantsManifest & { error?: string };
      if (!res.ok) throw new Error(data.error || 'Variant render failed');
      setVariants(data);
      setStatusTone('ready');
      setStatusText(`Rendered ${data.exports.length} variant export(s)`);
    } catch (err) {
      setStatusTone('error');
      setStatusText(err instanceof Error ? err.message : String(err));
    } finally {
      setVariantsBusy(false);
    }
  }, [session, dryReviewApproved, selectedVariants, activeCuesRunId]);

  const runPipelineRender = useCallback(
    async (
      targetRig: StudioRigState,
      targetStages: PipelineStagesState,
      opts: { crossfadeIfPlaying: boolean; autoSwitch: boolean },
    ) => {
      if (!session) return;
      const reqId = ++captureSeqRef.current;
      setPipelineBusy(true);
      setStatusTone('busy');
      const activeNames = [
        targetStages.micEnabled ? 'Mic' : null,
        targetStages.roomEnabled ? 'Room' : null,
        targetStages.pluginSlot.enabled && targetStages.pluginSlot.pluginId !== 'none'
          ? targetStages.pluginSlot.pluginId
          : null,
      ].filter(Boolean);
      setStatusText(
        activeNames.length === 0
          ? 'Rendering pipeline (all post-dry stages bypassed · bit-identical dry pass-through)…'
          : `Rendering post-dry pipeline in Rust (${activeNames.join(' → ')})…`,
      );
      try {
        const res = await fetch(`/api/sessions/${session.id}/pipeline-render`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            micEnabled: targetStages.micEnabled,
            roomEnabled: targetStages.roomEnabled,
            rigConfig: toRustCaptureConfig(targetRig),
            pluginSlot: serializePluginSlotForRust(targetStages.pluginSlot),
            cuesRunId: activeCuesRunId,
          }),
        });
        const data = (await res.json()) as PipelineRenderResponse & { error?: string };
        if (!res.ok) throw new Error(data.error || 'Rust pipeline render failed');
        const decoded = await decodeWavUrl(data.wavUrl);
        if (reqId !== captureSeqRef.current) return;

        setCapturedBuffer(decoded);
        setPipelineResult(data);
        if (opts.autoSwitch) {
          setTakeMode('captured');
          if (isPlayingRef.current) {
            crossfadePlaybackToBuffer(decoded);
          }
        } else if (
          opts.crossfadeIfPlaying &&
          isPlayingRef.current &&
          takeModeRef.current === 'captured'
        ) {
          crossfadePlaybackToBuffer(decoded);
        }

        setStatusTone('ready');
        setStatusText(
          data.summary.bit_identical_to_dry
            ? `Pipeline output bit-identical to cached Dry Take (SHA ${data.wav_sha256.slice(0, 10)})`
            : `Pipeline ready (${activeNames.join(' + ')} · SHA ${data.wav_sha256.slice(0, 10)})`,
        );
      } catch (err) {
        if (reqId !== captureSeqRef.current) return;
        setStatusTone('error');
        setStatusText(err instanceof Error ? err.message : String(err));
      } finally {
        if (reqId === captureSeqRef.current) {
          setPipelineBusy(false);
        }
      }
    },
    [session, activeCuesRunId, decodeWavUrl, crossfadePlaybackToBuffer, setTakeMode, isPlayingRef, takeModeRef],
  );

  const handleRerenderCapture = useCallback(() => {
    void runPipelineRender(rig, pipelineStages, { crossfadeIfPlaying: true, autoSwitch: true });
  }, [rig, pipelineStages, runPipelineRender]);

  // Debounced Live Preview: watches ONLY acoustic fingerprint + pipeline stage toggles (never display-only changes)
  const acousticFingerprint = useMemo(() => computeAcousticFingerprint(rig), [rig]);
  const pipelineStageKey = useMemo(
    () =>
      `${pipelineStages.micEnabled ? 1 : 0}|${pipelineStages.roomEnabled ? 1 : 0}|${pipelineStages.pluginSlot.enabled ? pipelineStages.pluginSlot.pluginId : 'off'}|${pipelineStages.pluginSlot.amount}|${pipelineStages.pluginSlot.eqLowDb}|${pipelineStages.pluginSlot.eqHighDb}|${pipelineStages.pluginSlot.ceilingDbfs}`,
    [pipelineStages],
  );
  const lastPreviewFpRef = useRef<string>('');
  useEffect(() => {
    if (!livePreview || !session) return;
    const combinedKey = `${acousticFingerprint}|${pipelineStageKey}`;
    if (lastPreviewFpRef.current === combinedKey && pipelineResult) return;
    const timer = window.setTimeout(() => {
      lastPreviewFpRef.current = combinedKey;
      void runPipelineRender(rig, pipelineStages, { crossfadeIfPlaying: true, autoSwitch: false });
    }, 320);
    return () => window.clearTimeout(timer);
    // Intentionally keyed on acousticFingerprint + pipelineStageKey so display-only rig edits never trigger renders
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [acousticFingerprint, pipelineStageKey, livePreview, session]);

  const handleToggleDryCriterion = useCallback((key: DryCriterionKey) => {
    setDryChecklist((prev) => {
      const next = { ...prev, [key]: !prev[key] };
      const passed = countPassedDryCriteria(next);
      if (isDryListeningPassed(next)) {
        setStatusTone('ready');
        setStatusText('Dry listening gate passed (5/5) · Stage 4 post-processing unlocked');
      } else {
        setLivePreview(false);
        setStatusTone('ready');
        setStatusText(
          `Dry listening check (${passed}/5) · Pass all 5 criteria before Stage 4 post-processing`,
        );
      }
      return next;
    });
  }, []);

  const openDryReviewDock = useCallback(() => {
    setDockCollapsed(false);
    setDockTab('review');
  }, [setDockCollapsed]);

  // Global keyboard shortcuts (Space, Undo, Redo, Open)
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement | null)?.tagName;
      const inInput = tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
      if (e.code === 'Space' && !inInput && session) {
        e.preventDefault();
        handleTogglePlay();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'z') {
        e.preventDefault();
        if (e.shiftKey) handleRedo();
        else handleUndo();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'o') {
        e.preventDefault();
        fileInputRef.current?.click();
      } else if (e.key === 'Escape') {
        setOpenMenu(null);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [session, handleTogglePlay, handleUndo, handleRedo]);

  const effectiveGains = useMemo(() => computeEffectiveLayerGains(layerChannels), [layerChannels]);
  void effectiveGains;

  const viewedLayerChannel = viewedLayerId
    ? layerChannels.find((c) => c.id === viewedLayerId) ?? null
    : null;
  const displayedResultBuffer =
    viewedLayerId && layerBuffers[viewedLayerId]
      ? layerBuffers[viewedLayerId] ?? regenBuffer
      : takeMode === 'difference'
        ? diffBuffer
        : takeMode === 'captured' && capturedBuffer
          ? capturedBuffer
          : regenBuffer;
  const resultTrackLabel =
    takeMode === 'difference'
      ? 'Difference (Regenerated − Original)'
      : takeMode === 'captured' && capturedBuffer
        ? `Captured Take (Rust 6-Band Room & Mic · SHA ${captureResult?.sha256.slice(0, 8) ?? '—'})`
        : 'New Dry Take (Synthesized From Cues Alone · No Original Blended)';

  const zoomBy = (factor: number) => {
    if (durationS <= 0) return;
    const span = viewEndS - viewStartS;
    const center = (viewStartS + viewEndS) / 2;
    const nextSpan = Math.max(0.15, Math.min(durationS, span * factor));
    let ns = center - nextSpan / 2;
    let ne = center + nextSpan / 2;
    if (ns < 0) {
      ns = 0;
      ne = Math.min(durationS, nextSpan);
    }
    if (ne > durationS) {
      ne = durationS;
      ns = Math.max(0, durationS - nextSpan);
    }
    setViewStartS(ns);
    setViewEndS(ne);
  };

  return (
    <div
      className="h-screen w-screen overflow-hidden flex flex-col bg-[#1e1e1e] text-[#dcdcdc] select-none"
      onClick={() => openMenu && setOpenMenu(null)}
      onDragOver={(e) => {
        e.preventDefault();
        setIsDraggingFile(true);
      }}
      onDragLeave={() => setIsDraggingFile(false)}
      onDrop={(e) => {
        e.preventDefault();
        setIsDraggingFile(false);
        const file = e.dataTransfer.files?.[0];
        if (file) void handleUploadWavFile(file);
      }}
    >
      <input
        ref={fileInputRef}
        type="file"
        accept=".wav,audio/wav"
        className="hidden"
        onChange={(e) => {
          const f = e.target.files?.[0];
          if (f) void handleUploadWavFile(f);
          e.target.value = '';
        }}
      />

      {/* 1. Native Desktop Menu Bar (ArtCraft Pro 28px) */}
      <header
        className="h-7 bg-[#2c2c2c] border-b border-[#3c3c3c] px-2 flex items-center justify-between text-[12px] shrink-0 relative z-30"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-0.5">
          <div className="flex items-center gap-1.5 pr-3 mr-1 border-r border-[#3f3f3f]">
            <span className="w-2.5 h-2.5 bg-[#378ef0] inline-block" />
            <span className="font-semibold tracking-tight text-[#efefef]">SpectraCraft</span>
          </div>

          {(
            [
              ['file', 'File'],
              ['edit', 'Edit'],
              ['audio', 'Audio'],
              ['view', 'View'],
              ['room', 'Room'],
            ] as [Exclude<MenuId, null>, string][]
          ).map(([id, label]) => (
            <div key={id} className="relative">
              <button
                type="button"
                onClick={() => setOpenMenu(openMenu === id ? null : id)}
                className={`px-2 py-0.5 text-[12px] ${
                  openMenu === id ? 'bg-[#378ef0] text-white' : 'text-[#d0d0d0] hover:bg-[#3a3a3a]'
                }`}
              >
                {label}
              </button>

              {openMenu === id && (
                <div className="absolute left-0 top-full mt-px w-60 bg-[#2a2a2a] border border-[#454545] py-1 shadow-xl z-40 text-[12px]">
                  {id === 'file' && (
                    <>
                      <MenuItem label="Open Single-Source WAV…" shortcut="Ctrl+O" onClick={() => { setOpenMenu(null); fileInputRef.current?.click(); }} />
                      <MenuItem label="Open Slow Solo Reference (48 kHz)" onClick={() => void handleOpenReference()} />
                      <MenuSep />
                      <MenuItem
                        label="Export Dry WAV / Cues / Variants…"
                        disabled={!session}
                        onClick={() => { setOpenMenu(null); setExportModalOpen(true); }}
                      />
                      <MenuSep />
                      <MenuItem label="Close File" disabled={!session} onClick={handleCloseFile} />
                    </>
                  )}
                  {id === 'edit' && (
                    <>
                      <MenuItem label="Undo" shortcut="Ctrl+Z" disabled={rigHistory.past.length === 0} onClick={handleUndo} />
                      <MenuItem label="Redo" shortcut="Ctrl+Shift+Z" disabled={rigHistory.future.length === 0} onClick={handleRedo} />
                      <MenuSep />
                      <MenuItem label="Clear Selection" disabled={!selection} onClick={() => { setSelection(null); setOpenMenu(null); }} />
                      <MenuItem label="Reset Room & Rig" onClick={() => { handleUpdateRig(makeRoomPreset('small'), true); setOpenMenu(null); }} />
                    </>
                  )}
                  {id === 'audio' && (
                    <>
                      <MenuItem label="Synthesize Dry WAV From Cues" disabled={!session} onClick={() => { setOpenMenu(null); void handleRenderEditedCues(); }} />
                      <MenuItem label="Analyze Inspection Layers" disabled={!session} onClick={() => { setOpenMenu(null); setDockCollapsed(false); setDockTab('layers'); void handleAnalyzeLayers(); }} />
                      <MenuItem label="Dry Listening Gate (5 Checks)" disabled={!session} onClick={() => { setOpenMenu(null); openDryReviewDock(); }} />
                      <MenuSep />
                      <MenuItem
                        label={dryReviewApproved ? 'Rerender Capture (Stage 4)' : 'Rerender Capture (Locked 0–4/5)'}
                        disabled={!session || !dryReviewApproved}
                        onClick={() => { setOpenMenu(null); handleRerenderCapture(); }}
                      />
                    </>
                  )}
                  {id === 'view' && (
                    <>
                      <MenuItem label="Spectrograms" checked={displayMode === 'spectrogram'} onClick={() => { setDisplayMode('spectrogram'); setOpenMenu(null); }} />
                      <MenuItem label="Waveforms" checked={displayMode === 'waveform'} onClick={() => { setDisplayMode('waveform'); setOpenMenu(null); }} />
                      <MenuItem label="Both (Spectrogram + Waveform)" checked={displayMode === 'both'} onClick={() => { setDisplayMode('both'); setOpenMenu(null); }} />
                      <MenuSep />
                      <MenuItem label="Cue Evidence Overlay (Visual Only)" checked={showEvidenceOverlay} onClick={() => { setShowEvidenceOverlay((v) => !v); setOpenMenu(null); }} />
                      <MenuItem label="Level-Match A/B Playback" checked={levelMatchAb} onClick={() => { setLevelMatchAb((v) => !v); setOpenMenu(null); }} />
                      <MenuSep />
                      <MenuItem label="Fit Recording" disabled={!session} onClick={() => { setViewStartS(0); setViewEndS(durationS); setOpenMenu(null); }} />
                      <MenuItem label="Zoom to Selection" disabled={!selection} onClick={() => { if (selection) { setViewStartS(selection[0]); setViewEndS(selection[1]); } setOpenMenu(null); }} />
                      <MenuItem label="Jump to 25.50 s" disabled={durationS < 26} onClick={() => { handleSeek(25.5); setViewStartS(24.5); setViewEndS(Math.min(durationS, 26.5)); setOpenMenu(null); }} />
                      <MenuSep />
                      <MenuItem label="Toggle Bottom Dock" checked={!dockCollapsed} onClick={() => { setDockCollapsed((c) => !c); setOpenMenu(null); }} />
                      <MenuItem label="Toggle Capture Inspector" checked={inspectorOpen} onClick={() => { setInspectorOpen((o) => !o); setOpenMenu(null); }} />
                      <MenuItem label="Toggle Room Views" checked={layout.spatialOpen} onClick={() => { setLayout((l) => ({ ...l, spatialOpen: !l.spatialOpen })); setOpenMenu(null); }} />
                      <MenuSep />
                      <MenuItem label="Room View: Plans (Top + Front)" checked={layout.spatialMode === 'plans'} onClick={() => { setLayout((l) => ({ ...l, spatialOpen: true, spatialMode: 'plans' })); setOpenMenu(null); }} />
                      <MenuItem label="Room View: 3D Grid" checked={layout.spatialMode === '3d'} onClick={() => { setLayout((l) => ({ ...l, spatialOpen: true, spatialMode: '3d' })); setOpenMenu(null); }} />
                      <MenuItem label="Room View: Split (Plans + 3D)" checked={layout.spatialMode === 'split'} onClick={() => { setLayout((l) => ({ ...l, spatialOpen: true, spatialMode: 'split' })); setOpenMenu(null); }} />
                      <MenuSep />
                      <MenuItem
                        label={layout.inspectorFloat.floating ? 'Dock Capture Inspector' : 'Undock Capture Inspector'}
                        onClick={() => { setLayout((l) => setPaneFloating(l, 'inspector', !l.inspectorFloat.floating)); setOpenMenu(null); }}
                      />
                      <MenuItem
                        label={layout.spatialFloat.floating ? 'Dock Room Views' : 'Undock Room Views'}
                        onClick={() => { setLayout((l) => setPaneFloating(l, 'spatial', !l.spatialFloat.floating)); setOpenMenu(null); }}
                      />
                      <MenuItem
                        label="Reset Workspace Layout"
                        onClick={() => { setLayout(createDefaultWorkspaceLayout()); setOpenMenu(null); }}
                      />
                      <MenuSep />
                      <MenuItem
                        label="Resynthesis Lineage & Scope…"
                        onClick={() => { setLineageModalOpen(true); setOpenMenu(null); }}
                      />
                    </>
                  )}
                  {id === 'room' && (
                    <>
                      {(
                        [
                          ['small', 'Small Room (10 × 10 × 10 ft)'],
                          ['medium', 'Medium Room (20 × 20 × 10 ft)'],
                          ['large', 'Large Room (50 × 50 × 10 ft)'],
                          ['open_field', 'Open Field (Ground Only)'],
                          ['direct', 'Direct (No Room)'],
                        ] as [RoomPresetKey, string][]
                      ).map(([preset, title]) => (
                        <MenuItem
                          key={preset}
                          label={title}
                          checked={rig.preset === preset}
                          onClick={() => {
                            handleUpdateRig(makeRoomPreset(preset, rig), true);
                            setOpenMenu(null);
                          }}
                        />
                      ))}
                    </>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>

        {/* Quiet active file metadata on right */}
        <div className="font-mono text-[11px] text-[#9b9b9b] tabular-nums truncate max-w-[460px]">
          {session
            ? `${session.label} · ${(sampleRate / 1000).toFixed(1)} kHz mono · ${durationS.toFixed(3)} s`
            : 'No file open'}
        </div>
      </header>

      {/* 2. Compact Transport & Tool Options Strip (40px) */}
      <div className="h-10 bg-[#262626] border-b border-[#3c3c3c] px-2 flex items-center justify-between gap-2 shrink-0 overflow-x-auto">
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            onClick={() => fileInputRef.current?.click()}
            className="h-7 px-2 bg-[#323232] hover:bg-[#3c3c3c] border border-[#444444] text-[#e0e0e0] flex items-center gap-1.5 text-[11px]"
            title="Open single-source WAV file (Ctrl+O)"
          >
            <FolderOpen className="w-3.5 h-3.5" />
            <span>Open</span>
          </button>

          <div className="flex items-center border border-[#424242]">
            <button
              type="button"
              disabled={rigHistory.past.length === 0}
              onClick={handleUndo}
              className="h-7 w-7 bg-[#303030] hover:bg-[#3b3b3b] disabled:opacity-35 flex items-center justify-center border-r border-[#424242]"
              title="Undo (Ctrl+Z)"
            >
              <Undo2 className="w-3.5 h-3.5" />
            </button>
            <button
              type="button"
              disabled={rigHistory.future.length === 0}
              onClick={handleRedo}
              className="h-7 w-7 bg-[#303030] hover:bg-[#3b3b3b] disabled:opacity-35 flex items-center justify-center"
              title="Redo (Ctrl+Shift+Z)"
            >
              <Redo2 className="w-3.5 h-3.5" />
            </button>
          </div>

          <div className="w-px h-5 bg-[#3c3c3c] mx-0.5" />

          {/* Transport Controls */}
          <button
            type="button"
            disabled={!session || loadingAudio}
            onClick={() => handleSeek(0)}
            className="h-7 w-7 bg-[#323232] hover:bg-[#3c3c3c] disabled:opacity-35 border border-[#444444] flex items-center justify-center"
            title="Rewind to start"
          >
            <SkipBack className="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            disabled={!session || loadingAudio}
            onClick={handleTogglePlay}
            className={`h-7 px-2.5 border flex items-center gap-1.5 text-[11px] font-medium disabled:opacity-35 ${
              isPlaying
                ? 'bg-[#378ef0] border-[#378ef0] text-white'
                : 'bg-[#323232] hover:bg-[#3d3d3d] border-[#444444] text-[#e8e8e8]'
            }`}
            title="Play / Pause (Space)"
          >
            {isPlaying ? <Pause className="w-3.5 h-3.5" /> : <Play className="w-3.5 h-3.5" />}
            <span>{isPlaying ? 'Pause' : 'Play'}</span>
          </button>

          <div className="h-7 px-2 bg-[#1b1b1b] border border-[#3a3a3a] flex items-center font-mono text-[11px] tabular-nums text-[#e4e4e4]">
            {playheadS.toFixed(3)}s
            <span className="text-[#6e6e6e] ml-1">/ {durationS.toFixed(3)}s</span>
          </div>

          {durationS >= 26 && (
            <button
              type="button"
              onClick={() => {
                handleSeek(25.5);
                setViewStartS(24.5);
                setViewEndS(Math.min(durationS, 26.5));
              }}
              className="h-7 px-2 bg-[#323232] hover:bg-[#3c3c3c] border border-[#444444] font-mono text-[11px] text-[#d6d6d6]"
              title="Jump to 25.50 s transition window"
            >
              25.50 s
            </button>
          )}

          <div className="w-px h-5 bg-[#3c3c3c] mx-0.5" />

          {/* Time-aligned & Level-matched A/B Take Selector */}
          <div className="flex items-center border border-[#444444]">
            {(
              [
                ['original', 'Original', !session],
                ['regenerated', 'Dry Take', !session],
                ['captured', 'Captured', !session || !capturedBuffer],
                ['difference', 'Difference', !session],
              ] as [TakeMode, string, boolean][]
            ).map(([mode, label, disabled]) => (
              <button
                key={mode}
                type="button"
                disabled={disabled}
                onClick={() => handleSwitchTake(mode)}
                className={`h-7 px-2.5 text-[11px] border-r last:border-r-0 border-[#444444] disabled:opacity-35 ${
                  takeMode === mode
                    ? 'bg-[#378ef0] text-white font-medium'
                    : 'bg-[#303030] text-[#c8c8c8] hover:bg-[#3a3a3a]'
                }`}
              >
                {label}
              </button>
            ))}
          </div>

          <button
            type="button"
            disabled={!session}
            onClick={() => setLevelMatchAb((v) => !v)}
            title="Apply Rust-measured RMS level match during Original vs Dry Take A/B playback"
            className={`h-7 px-2 border font-mono text-[10px] disabled:opacity-35 ${
              levelMatchAb
                ? 'bg-[#21332c] border-[#3a7c63] text-[#6cd4b4]'
                : 'bg-[#2f2f2f] border-[#444444] text-[#9e9e9e]'
            }`}
          >
            Match dB
          </button>

          <div className="w-px h-5 bg-[#3c3c3c] mx-0.5" />

          {/* Display Mode & Zoom */}
          <div className="flex items-center border border-[#444444]">
            {(
              [
                ['spectrogram', 'Spec'],
                ['waveform', 'Wave'],
                ['both', 'Both'],
              ] as [DisplayMode, string][]
            ).map(([m, label]) => (
              <button
                key={m}
                type="button"
                onClick={() => setDisplayMode(m)}
                className={`h-7 px-2 text-[11px] border-r last:border-r-0 border-[#444444] ${
                  displayMode === m
                    ? 'bg-[#464646] text-white font-medium'
                    : 'bg-[#2f2f2f] text-[#b4b4b4] hover:bg-[#383838]'
                }`}
              >
                {label}
              </button>
            ))}
          </div>

          <button
            type="button"
            disabled={!session}
            onClick={() => setShowEvidenceOverlay((v) => !v)}
            title="Toggle visual-only cue evidence overlay (strong / weak / uncertain — never proof of audio)"
            className={`h-7 px-2 border flex items-center gap-1 text-[11px] disabled:opacity-35 ${
              showEvidenceOverlay
                ? 'bg-[#378ef0] border-[#378ef0] text-white'
                : 'bg-[#303030] border-[#444444] text-[#c2c2c2] hover:bg-[#3a3a3a]'
            }`}
          >
            <Eye className="w-3.5 h-3.5" />
            <span>Evidence</span>
          </button>

          <div className="flex items-center border border-[#444444]">
            <button
              type="button"
              disabled={!session}
              onClick={() => zoomBy(0.5)}
              className="h-7 w-7 bg-[#303030] hover:bg-[#3a3a3a] disabled:opacity-35 flex items-center justify-center border-r border-[#444444]"
              title="Zoom in"
            >
              <ZoomIn className="w-3.5 h-3.5" />
            </button>
            <button
              type="button"
              disabled={!session}
              onClick={() => zoomBy(2.0)}
              className="h-7 w-7 bg-[#303030] hover:bg-[#3a3a3a] disabled:opacity-35 flex items-center justify-center border-r border-[#444444]"
              title="Zoom out"
            >
              <ZoomOut className="w-3.5 h-3.5" />
            </button>
            <button
              type="button"
              disabled={!session}
              onClick={() => {
                setViewStartS(0);
                setViewEndS(durationS);
              }}
              className="h-7 px-2 bg-[#303030] hover:bg-[#3a3a3a] disabled:opacity-35 text-[11px]"
              title="Fit full recording"
            >
              Fit
            </button>
          </div>
        </div>

        {/* Right side: Scale Hz, Contrast (visual only), Bit Depth, Synthesize & Export */}
        <div className="flex items-center gap-1.5 shrink-0">
          <select
            value={displayMaxHz}
            onChange={(e) => setDisplayMaxHz(Number(e.target.value))}
            aria-label="Spectrogram Max Hz"
            className="h-7 px-1.5 bg-[#1f1f1f] border border-[#424242] font-mono text-[11px] text-[#d6d6d6]"
          >
            <option value={8000}>8 kHz</option>
            <option value={12000}>12 kHz</option>
            <option value={16000}>16 kHz</option>
            <option value={20000}>20 kHz</option>
            <option value={24000}>24 kHz</option>
            <option value={32000}>32 kHz</option>
          </select>

          <select
            value={spectrogramDbFloor}
            onChange={(e) => setSpectrogramDbFloor(Number(e.target.value))}
            aria-label="Spectrogram Contrast Floor"
            title="Visual-only spectrogram floor contrast (never alters output audio)"
            className="h-7 px-1.5 bg-[#1f1f1f] border border-[#424242] font-mono text-[11px] text-[#d6d6d6]"
          >
            <option value={-78}>-78 dB</option>
            <option value={-96}>-96 dB</option>
            <option value={-112}>-112 dB</option>
          </select>

          <select
            value={previewBitDepth}
            onChange={(e) => setPreviewBitDepth(Number(e.target.value) as 32 | 24 | 16)}
            aria-label="Preview Bit Depth"
            className="h-7 px-1.5 bg-[#1f1f1f] border border-[#424242] font-mono text-[11px] text-[#d6d6d6]"
          >
            <option value={32}>32f</option>
            <option value={24}>24b</option>
            <option value={16}>16b</option>
          </select>

          <button
            type="button"
            disabled={!session || cuesBusy}
            onClick={() => void handleRenderEditedCues()}
            className="h-7 px-2.5 bg-[#323232] hover:bg-[#3c3c3c] disabled:opacity-35 border border-[#464646] flex items-center gap-1.5 text-[11px]"
            title="Synthesize a fresh dry WAV from cues alone"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${cuesBusy ? 'animate-spin' : ''}`} />
            <span>Synthesize</span>
          </button>

          <button
            type="button"
            disabled={!session}
            onClick={() => setExportModalOpen(true)}
            className="h-7 px-2.5 bg-[#378ef0] hover:bg-[#2c7ad4] disabled:bg-[#343434] disabled:text-[#6e6e6e] text-white font-medium flex items-center gap-1.5 text-[11px]"
          >
            <Download className="w-3.5 h-3.5" />
            <span>Export…</span>
          </button>
        </div>
      </div>

      {/* 2b. Compact Clean 5-Stage SpectraCraft Pipeline Bar (24px) */}
      <div className="h-6 bg-[#212121] border-b border-[#363636] px-2.5 flex items-center justify-between text-[11px] font-mono shrink-0">
        <div className="flex items-center gap-2">
          <StageStep
            num="1"
            label="Dry Take (Cached)"
            done={Boolean(session && regenBuffer)}
            active={Boolean(session && dockTab === 'shape')}
            onClick={() => {
              setInspectorOpen(true);
              setInspectorTab('source');
              if (session) {
                setDockCollapsed(false);
                setDockTab('shape');
              }
            }}
          />
          <span className="text-[#505050]">→</span>
          <StageStep
            num="2"
            label={`Region Edits (${regionHistory.regions.length})`}
            done={regionHistory.regions.some((r) => r.appliedCount > 0)}
            active={dockTab === 'regions' && !dockCollapsed}
            onClick={() => {
              setActiveTool('marquee');
              if (session) {
                setDockCollapsed(false);
                setDockTab('regions');
              }
            }}
          />
          <span className="text-[#505050]">→</span>
          <button
            type="button"
            onClick={() =>
              setPipelineStages((p) => ({ ...p, micEnabled: !p.micEnabled }))
            }
            className={`text-[10px] transition-colors ${
              pipelineStages.micEnabled ? 'text-[#8bc2ff] font-semibold' : 'text-[#848484] hover:text-[#cccccc]'
            }`}
            title="Toggle Stage 3 Optional Microphone Stage"
          >
            3. Mic ({pipelineStages.micEnabled ? 'ON' : 'OFF'})
          </button>
          <span className="text-[#505050]">→</span>
          <button
            type="button"
            onClick={() =>
              setPipelineStages((p) => ({ ...p, roomEnabled: !p.roomEnabled }))
            }
            className={`text-[10px] transition-colors ${
              pipelineStages.roomEnabled ? 'text-[#4cc6ac] font-semibold' : 'text-[#848484] hover:text-[#cccccc]'
            }`}
            title="Toggle Stage 4 Optional Space / Room Stage (10×10×10 ft padded)"
          >
            4. Room ({pipelineStages.roomEnabled ? 'ON' : 'OFF'})
          </button>
          <span className="text-[#505050]">→</span>
          <button
            type="button"
            onClick={() =>
              setPipelineStages((p) => ({
                ...p,
                pluginSlot: {
                  ...p.pluginSlot,
                  enabled: !p.pluginSlot.enabled,
                  pluginId:
                    !p.pluginSlot.enabled && p.pluginSlot.pluginId === 'none'
                      ? 'gentle_eq'
                      : p.pluginSlot.pluginId,
                },
              }))
            }
            className={`text-[10px] transition-colors ${
              pipelineStages.pluginSlot.enabled && pipelineStages.pluginSlot.pluginId !== 'none'
                ? 'text-[#f0b44c] font-semibold'
                : 'text-[#848484] hover:text-[#cccccc]'
            }`}
            title="Toggle Stage 5 Optional Plugin Finishing Slot"
          >
            5. Plugin (
            {pipelineStages.pluginSlot.enabled && pipelineStages.pluginSlot.pluginId !== 'none'
              ? pipelineStages.pluginSlot.pluginId.replace('_', ' ')
              : 'OFF'}
            )
          </button>
        </div>

        <div className="text-[10px] text-[#7c7c7c]">
          Single source · Zero original waveform blending · Every post-dry stage bypassable
        </div>
      </div>

      {/* 3. Main ArtCraft Three-Column Desktop Body */}
      <div className="flex-1 min-h-0 flex relative">
        {/* Left Vertical ArtCraft Tool Rail (44px) */}
        <nav
          aria-label="Workspace tools"
          className="w-11 bg-[#262626] border-r border-[#3c3c3c] flex flex-col items-center py-1.5 gap-1 shrink-0"
        >
          <ToolButton
            icon={<MousePointer2 className="w-4 h-4" />}
            label="Scrub / Playhead tool"
            active={activeTool === 'scrub'}
            onClick={() => setActiveTool('scrub')}
          />
          <ToolButton
            icon={<BoxSelect className="w-4 h-4" />}
            label="Rectangular Marquee tool (Stage 2 Dry Take)"
            active={activeTool === 'marquee'}
            onClick={() => setActiveTool('marquee')}
          />
          <ToolButton
            icon={<Crosshair className="w-4 h-4" />}
            label="In/Out Range selection tool (Stage 2 Dry Take)"
            active={activeTool === 'range'}
            onClick={() => setActiveTool('range')}
          />
          <ToolButton
            icon={<Search className="w-4 h-4" />}
            label="Zoom tool (Click in, Shift+Click out)"
            active={activeTool === 'zoom'}
            onClick={() => setActiveTool('zoom')}
          />

          <div className="w-6 h-px bg-[#3c3c3c] my-1" />

          <ToolButton
            icon={<Scissors className="w-4 h-4" />}
            label="Stage 2 Surgical Region Edits (Dry)"
            active={!dockCollapsed && dockTab === 'regions'}
            onClick={() => {
              setDockTab('regions');
              setDockCollapsed(false);
            }}
          />
          <ToolButton
            icon={<Layers className="w-4 h-4" />}
            label="Editable Layers dock"
            active={!dockCollapsed && dockTab === 'layers'}
            onClick={() => {
              setDockTab('layers');
              setDockCollapsed(false);
            }}
          />
          <ToolButton
            icon={<Music2 className="w-4 h-4" />}
            label="Performance Cues dock"
            active={!dockCollapsed && dockTab === 'cues'}
            onClick={() => {
              setDockTab('cues');
              setDockCollapsed(false);
            }}
          />
          <ToolButton
            icon={<Wrench className="w-4 h-4" />}
            label="Local Gap Repair dock"
            active={!dockCollapsed && dockTab === 'repair'}
            onClick={() => {
              setDockTab('repair');
              setDockCollapsed(false);
            }}
          />
          <ToolButton
            icon={<CheckCircle2 className="w-4 h-4" />}
            label="Dry Listening Gate"
            active={!dockCollapsed && dockTab === 'review'}
            onClick={openDryReviewDock}
          />
          <ToolButton
            icon={<Box className="w-4 h-4" />}
            label="Toggle Capture & Pipeline Inspector"
            active={inspectorOpen}
            onClick={() => setInspectorOpen((o) => !o)}
          />
        </nav>

        {/* Main Rearrangeable ArtCraft Workspace Body (Panes + Resizable Splitters + Bottom Dock) */}
        <main className="flex-1 min-w-0 min-h-0 flex flex-col bg-[#1a1a1a] relative">
          {(() => {
            const dockedPanes: WorkspacePaneId[] = layout.paneOrder.filter((pane) => {
              if (pane === 'inspector') return layout.inspectorOpen && !layout.inspectorFloat.floating;
              if (pane === 'spatial') return layout.spatialOpen && !layout.spatialFloat.floating;
              return true;
            });

            const inspectorNode = (isFloating: boolean) => {
              const idx = dockedPanes.indexOf('inspector');
              return (
                <StudioInspector
                  rig={rig}
                  catalogs={catalogs}
                  activeTab={inspectorTab}
                  onTabChange={setInspectorTab}
                  onUpdateRig={handleUpdateRig}
                  livePreview={livePreview}
                  onToggleLivePreview={setLivePreview}
                  onRerenderCapture={handleRerenderCapture}
                  captureBusy={captureBusy}
                  captureResult={captureResult}
                  acousticAnalysis={
                    rustAcousticAnalysis ??
                    pipelineResult?.acoustic_analysis ??
                    captureResult?.acoustic_analysis ??
                    null
                  }
                  analysisFromRust={Boolean(
                    rustAcousticAnalysis ??
                      pipelineResult?.acoustic_analysis ??
                      captureResult?.acoustic_analysis,
                  )}
                  hasAudio={Boolean(session)}
                  onRegenerateDryWithSource={() => void handleRegenerateDryWithSource()}
                  sourceRegenBusy={sourceRegenBusy}
                  sourceRegenResult={sourceRegenResult}
                  voicing={voicing}
                  onChangeVoicing={setVoicing}
                  onReloadCatalogs={() => void handleReloadCatalogs()}
                  reloadingCatalogs={reloadingCatalogs}
                  pipelineStages={pipelineStages}
                  onChangePipelineStages={setPipelineStages}
                  onRenderPipeline={handleRerenderCapture}
                  pipelineBusy={pipelineBusy}
                  pipelineResult={pipelineResult}
                  dryReviewApproved={dryReviewApproved}
                  dryPassedCount={dryPassedCount}
                  onOpenDryReview={openDryReviewDock}
                  isFloating={isFloating}
                  canMoveLeft={!isFloating && idx > 0}
                  canMoveRight={!isFloating && idx >= 0 && idx < dockedPanes.length - 1}
                  onMovePane={(dir) => setLayout((l) => movePaneInOrder(l, 'inspector', dir))}
                  onToggleFloat={() =>
                    setLayout((l) => setPaneFloating(l, 'inspector', !l.inspectorFloat.floating))
                  }
                />
              );
            };

            const spatialNode = (isFloating: boolean) => {
              const idx = dockedPanes.indexOf('spatial');
              return (
                <StudioPlanCanvas
                  rig={rig}
                  catalogs={catalogs}
                  onMoveMic={(micIdx: number, worldPos: Vec3, commit: boolean) =>
                    handleUpdateRig(setMicWorldPosM(rig, micIdx, worldPos), commit)
                  }
                  onSelectMic={(micIdx: number) =>
                    handleUpdateRig({ ...rig, selectedMic: micIdx }, false)
                  }
                  onMoveSource={(worldPos: Vec3, commit: boolean) =>
                    handleUpdateRig(setSourcePosM(rig, worldPos), commit)
                  }
                  onTogglePatterns={(show: boolean) =>
                    handleUpdateRig({ ...rig, showPatterns: show }, false)
                  }
                  onToggleGuidanceOverlay={(show: boolean) =>
                    handleUpdateRig({ ...rig, showGuidanceOverlay: show }, false)
                  }
                  onToggleGridSnap={(snap: boolean) =>
                    handleUpdateRig({ ...rig, gridSnap: snap }, false)
                  }
                  spatialMode={layout.spatialMode}
                  onChangeSpatialMode={(m) => setLayout((l) => ({ ...l, spatialMode: m }))}
                  isFloating={isFloating}
                  canMoveLeft={!isFloating && idx > 0}
                  canMoveRight={!isFloating && idx >= 0 && idx < dockedPanes.length - 1}
                  onMovePane={(dir) => setLayout((l) => movePaneInOrder(l, 'spatial', dir))}
                  onToggleFloat={() =>
                    setLayout((l) => setPaneFloating(l, 'spatial', !l.spatialFloat.floating))
                  }
                />
              );
            };

            const timelineNode = (
              <div className="flex-1 min-w-[260px] flex flex-col min-h-0">
                {!session ? (
                  <div className="flex-1 flex items-center justify-center p-6">
                    <div
                      className={`w-[392px] max-w-full bg-[#262626] border p-5 space-y-4 ${
                        isDraggingFile ? 'border-[#378ef0]' : 'border-[#3c3c3c]'
                      }`}
                    >
                      <div className="space-y-1">
                        <div className="text-[14px] font-semibold text-[#f0f0f0]">SpectraCraft</div>
                        <div className="text-[12px] text-[#9b9b9b]">
                          Open one isolated instrument, voice, or stem WAV to extract editable cues and synthesize a new dry take.
                        </div>
                      </div>

                      <div className="flex flex-col gap-2">
                        <button
                          type="button"
                          disabled={statusTone === 'busy'}
                          onClick={() => fileInputRef.current?.click()}
                          className="w-full h-8 bg-[#378ef0] hover:bg-[#2c7ad4] disabled:bg-[#3a3a3a] text-white font-medium text-[12px] flex items-center justify-center gap-2"
                        >
                          <FolderOpen className="w-4 h-4" />
                          <span>Open WAV…</span>
                        </button>
                        <button
                          type="button"
                          disabled={statusTone === 'busy'}
                          onClick={() => void handleOpenReference()}
                          className="w-full h-8 bg-[#323232] hover:bg-[#3c3c3c] disabled:opacity-50 border border-[#464646] text-[#dcdcdc] text-[12px]"
                        >
                          {statusTone === 'busy'
                            ? 'Loading Reference in Rust…'
                            : 'Open Reference WAV (Rust Engine)'}
                        </button>
                      </div>

                      <div className="pt-2 border-t border-[#363636] font-mono text-[10px] text-[#858585] flex items-center justify-between">
                        <span>One source · Mono WAV 8–96 kHz</span>
                        <span>Original never blended</span>
                      </div>
                    </div>
                  </div>
                ) : (
                  <AudioViewport
                    originalBuffer={origBuffer}
                    resultBuffer={displayedResultBuffer}
                    resultLabel={resultTrackLabel}
                    viewedLayerLabel={viewedLayerChannel?.label ?? null}
                    sampleRate={sampleRate}
                    durationS={durationS}
                    viewStartS={viewStartS}
                    viewEndS={viewEndS}
                    playheadS={playheadS}
                    selection={selection}
                    displayMode={displayMode}
                    displayMaxHz={displayMaxHz}
                    spectrogramDbFloor={spectrogramDbFloor}
                    showEvidenceOverlay={showEvidenceOverlay}
                    notes={session.notes}
                    frameEvidence={session.transitionEvidence}
                    activeTool={activeTool}
                    onSeek={handleSeek}
                    onSelectRange={setSelection}
                    onZoomViewport={(ns, ne) => {
                      setViewStartS(ns);
                      setViewEndS(ne);
                    }}
                    onCursorMove={setCursor}
                    dryRegions={regionHistory.regions}
                    selectedRegionId={regionHistory.selectedId}
                    onSelectDryRegion={(id) => setRegionHistory((prev) => selectDryRegion(prev, id))}
                    onCreateDryRegion={handleCreateDryRegion}
                    onTransformDryRegion={(id, next, commit) =>
                      setRegionHistory((prev) => updateDryRegion(prev, id, next, durationS, commit))
                    }
                  />
                )}
              </div>
            );

            return (
              <>
                <div className="flex-1 min-h-0 flex">
                  {dockedPanes.map((paneId, i) => {
                    const nextPane = dockedPanes[i + 1];
                    return (
                      <React.Fragment key={paneId}>
                        {paneId === 'inspector' && (
                          <div
                            style={{ width: `${layout.inspectorWidth}px` }}
                            className="shrink-0 min-h-0 flex flex-col"
                          >
                            {inspectorNode(false)}
                          </div>
                        )}
                        {paneId === 'timeline' && timelineNode}
                        {paneId === 'spatial' && (
                          <aside
                            aria-label="3D Capture Lab Room Views"
                            style={{ width: `${layout.spatialWidth}px` }}
                            className="shrink-0 min-h-0 bg-[#1c1d20] flex flex-col"
                          >
                            {spatialNode(false)}
                          </aside>
                        )}
                        {nextPane && (
                          <SplitterHandle
                            orientation="vertical"
                            label={`Resize ${paneId} / ${nextPane}`}
                            onDragDelta={(dx) => {
                              setLayout((prev) => {
                                let next = prev;
                                if (paneId === 'inspector') {
                                  next = resizeInspectorWidth(next, next.inspectorWidth + dx);
                                } else if (nextPane === 'inspector') {
                                  next = resizeInspectorWidth(next, next.inspectorWidth - dx);
                                }
                                if (paneId === 'spatial') {
                                  next = resizeSpatialWidth(next, next.spatialWidth + dx);
                                } else if (nextPane === 'spatial') {
                                  next = resizeSpatialWidth(next, next.spatialWidth - dx);
                                }
                                return next;
                              });
                            }}
                          />
                        )}
                      </React.Fragment>
                    );
                  })}
                </div>

                {/* Resizable Horizontal Splitter above Bottom Dock */}
                {!dockCollapsed && (
                  <SplitterHandle
                    orientation="horizontal"
                    label="Resize bottom dock height"
                    onDragDelta={(_dx, dy) =>
                      setLayout((prev) => resizeBottomDockHeight(prev, prev.bottomDockHeight - dy))
                    }
                  />
                )}

                <StudioBottomDock
                  collapsed={dockCollapsed}
                  bodyHeightPx={Math.max(80, layout.bottomDockHeight - 32)}
                  onToggleCollapse={() => setDockCollapsed((c) => !c)}
                  activeTab={dockTab}
                  onTabChange={setDockTab}
                  session={session}
                  selection={selection}
                  activeDryUrl={activeDryUrl}
                  activeCuesUrl={activeCuesUrl}
                  layerChannels={layerChannels}
                  viewedLayerId={viewedLayerId}
                  includeTexture={includeTexture}
                  onToggleIncludeTexture={setIncludeTexture}
                  onUpdateLayer={(id, patch) =>
                    setLayerChannels((prev) => prev.map((c) => (c.id === id ? { ...c, ...patch } : c)))
                  }
                  onToggleViewLayer={(id) => setViewedLayerId((curr) => (curr === id ? null : id))}
                  onAnalyzeLayers={() => void handleAnalyzeLayers()}
                  layersBusy={layersBusy}
                  hasLayerStems={Boolean(layerSession)}
                  notes={session?.notes ?? []}
                  cuePatches={cuePatches}
                  onUpdateCuePatch={(idx, patch) => setCuePatches((prev) => ({ ...prev, [idx]: patch }))}
                  onRenderEditedCues={() => void handleRenderEditedCues()}
                  cuesBusy={cuesBusy}
                  onRunGapRepair={(startS, gapMs) => void handleRunGapRepair(startS, gapMs)}
                  repairResult={repairResult}
                  repairBusy={repairBusy}
                  dryChecklist={dryChecklist}
                  onToggleDryCriterion={handleToggleDryCriterion}
                  onOpenExportModal={() => setExportModalOpen(true)}
                  referenceShape={referenceShape}
                  structuredPrompt={structuredPrompt}
                  activeDrySha256={
                    regionEditResult?.committed
                      ? regionEditResult.edited_sha256
                      : sourceRegenResult?.drySha256 ?? session?.verification.dry_candidate_sha256 ?? null
                  }
                  capturedSha256={pipelineResult?.wav_sha256 ?? captureResult?.sha256 ?? null}
                  onOpenLineageModal={() => setLineageModalOpen(true)}
                  onApplyCharacterGuess={handleApplyCharacterGuess}
                  dryRegions={regionHistory.regions}
                  selectedRegionId={regionHistory.selectedId}
                  canUndoRegion={regionHistory.past.length > 0}
                  canRedoRegion={regionHistory.future.length > 0}
                  onSelectDryRegion={(id) => setRegionHistory((prev) => selectDryRegion(prev, id))}
                  onQuickAddDryRegion={handleQuickAddDryRegion}
                  onUpdateDryRegion={(id, patch) =>
                    setRegionHistory((prev) => updateDryRegion(prev, id, patch, durationS, true))
                  }
                  onDeleteDryRegion={(id) => {
                    stopRegionLoop();
                    setRegionHistory((prev) => deleteDryRegion(prev, id));
                  }}
                  onUndoDryRegion={() => setRegionHistory((prev) => undoDryRegionHistory(prev))}
                  onRedoDryRegion={() => setRegionHistory((prev) => redoDryRegionHistory(prev))}
                  onPreviewDryRegion={(reg) => void runDryRegionEdit(reg, false)}
                  onApplyDryRegion={(reg) => void runDryRegionEdit(reg, true)}
                  regionEditBusy={regionEditBusy}
                  regionEditResult={regionEditResult}
                  regionPreviewPlaying={regionPreviewPlaying}
                  regionPreviewAb={regionPreviewAb}
                  onToggleRegionPreviewPlay={handleToggleRegionPreviewPlay}
                  onChangeRegionPreviewAb={handleChangeRegionPreviewAb}
                />

                {/* Undocked Floating Windows (pure visual chrome — preserve exact rig & playback state) */}
                {layout.inspectorOpen && layout.inspectorFloat.floating && (
                  <FloatingPanelWindow
                    title="Capture Lab Inspector"
                    bounds={layout.inspectorFloat}
                    onChangeBounds={(patch) =>
                      setLayout((l) => updateFloatingBounds(l, 'inspector', patch))
                    }
                    onRedock={() => setLayout((l) => setPaneFloating(l, 'inspector', false))}
                    onClose={() => setInspectorOpen(false)}
                  >
                    {inspectorNode(true)}
                  </FloatingPanelWindow>
                )}

                {layout.spatialOpen && layout.spatialFloat.floating && (
                  <FloatingPanelWindow
                    title="Room Views (Plans & 3D Grid)"
                    bounds={layout.spatialFloat}
                    onChangeBounds={(patch) =>
                      setLayout((l) => updateFloatingBounds(l, 'spatial', patch))
                    }
                    onRedock={() => setLayout((l) => setPaneFloating(l, 'spatial', false))}
                    onClose={() => setLayout((l) => ({ ...l, spatialOpen: false }))}
                  >
                    {spatialNode(true)}
                  </FloatingPanelWindow>
                )}
              </>
            );
          })()}
        </main>
      </div>

      {/* 4. Quiet Desktop Status Bar (24px) */}
      <footer className="h-6 bg-[#2c2c2c] border-t border-[#3c3c3c] px-2.5 flex items-center justify-between text-[11px] font-mono text-[#9b9b9b] shrink-0">
        <div className="flex items-center gap-3 truncate">
          <span
            className={
              statusTone === 'error'
                ? 'text-[#e07070]'
                : statusTone === 'busy'
                  ? 'text-[#e5a93c]'
                  : statusTone === 'ready'
                    ? 'text-[#4cc6ac]'
                    : 'text-[#9b9b9b]'
            }
          >
            {statusText}
          </span>
          <span className="text-[#565656]">|</span>
          <span className="uppercase text-[#8a8a8a]">Tool: {activeTool}</span>
          {cursor && (
            <>
              <span className="text-[#565656]">|</span>
              <span className="tabular-nums text-[#cfcfcf]">
                t: {cursor.timeS.toFixed(3)} s
                {cursor.freqHz !== null ? ` · f: ${Math.round(cursor.freqHz)} Hz` : ''}
              </span>
            </>
          )}
          {selection && (
            <>
              <span className="text-[#565656]">|</span>
              <span className="tabular-nums text-[#4cc6ac]">
                Sel: {selection[0].toFixed(2)}–{selection[1].toFixed(2)} s
              </span>
            </>
          )}
        </div>

        <div className="flex items-center gap-3 shrink-0 text-[#8a8a8a]">
          <span>Engine: Rust (spectracraft-mono)</span>
        </div>
      </footer>

      <ExportModal
        open={exportModalOpen}
        onClose={() => setExportModalOpen(false)}
        session={session}
        dryReviewApproved={dryReviewApproved}
        dryPassedCount={dryPassedCount}
        onOpenDryReview={openDryReviewDock}
        selectedVariants={selectedVariants}
        onChangeSelectedVariants={setSelectedVariants}
        onRenderVariants={() => void handleRenderVariants()}
        variantsBusy={variantsBusy}
        variants={variants}
        activeDryUrl={activeDryUrl}
        activeCuesUrl={activeCuesUrl}
      />

      <ResynthesisLineageModal
        open={lineageModalOpen}
        onClose={() => setLineageModalOpen(false)}
      />
    </div>
  );
}

function StageStep({
  num,
  label,
  done,
  active,
  onClick,
}: {
  num: string;
  label: string;
  done: boolean;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`text-[10px] transition-colors ${
        active
          ? 'text-[#378ef0] font-semibold'
          : done
            ? 'text-[#4cc6ac] hover:text-white'
            : 'text-[#848484] hover:text-[#cccccc]'
      }`}
    >
      {num}. {label}
    </button>
  );
}

function ToolButton({
  icon,
  label,
  active,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      aria-label={label}
      className={`w-8 h-8 flex items-center justify-center transition-colors ${
        active
          ? 'bg-[#378ef0] text-white'
          : 'text-[#adadad] hover:bg-[#363636] hover:text-[#f0f0f0]'
      }`}
    >
      {icon}
    </button>
  );
}

function MenuItem({
  label,
  shortcut,
  disabled,
  checked,
  onClick,
}: {
  label: string;
  shortcut?: string;
  disabled?: boolean;
  checked?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="w-full px-3 py-1 text-left flex items-center justify-between text-[#dcdcdc] hover:bg-[#378ef0] hover:text-white disabled:text-[#666666] disabled:hover:bg-transparent"
    >
      <span className="flex items-center gap-2">
        <span className="w-3 inline-block font-mono text-[10px]">{checked ? '✓' : ''}</span>
        <span>{label}</span>
      </span>
      {shortcut && <span className="font-mono text-[10px] opacity-70">{shortcut}</span>}
    </button>
  );
}

function MenuSep() {
  return <div className="my-1 border-t border-[#3c3c3c]" />;
}

/**
 * ArtCraft draggable splitter bar (5px hit area, 1px hairline seam, blue highlight on drag/hover).
 */
function SplitterHandle({
  orientation,
  label,
  onDragDelta,
}: {
  orientation: 'vertical' | 'horizontal';
  label: string;
  onDragDelta: (dx: number, dy: number) => void;
}) {
  const lastPosRef = useRef<{ x: number; y: number } | null>(null);
  const [dragging, setDragging] = useState(false);

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    lastPosRef.current = { x: e.clientX, y: e.clientY };
    setDragging(true);
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!lastPosRef.current) return;
    const dx = e.clientX - lastPosRef.current.x;
    const dy = e.clientY - lastPosRef.current.y;
    if (dx === 0 && dy === 0) return;
    lastPosRef.current = { x: e.clientX, y: e.clientY };
    onDragDelta(dx, dy);
  };

  const onPointerUp = () => {
    lastPosRef.current = null;
    setDragging(false);
  };

  const isVert = orientation === 'vertical';
  return (
    <div
      role="separator"
      aria-orientation={orientation}
      aria-label={label}
      title={label}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      className={`${
        isVert ? 'w-[5px] cursor-col-resize' : 'h-[5px] cursor-row-resize'
      } shrink-0 relative group bg-[#1e1e1e] select-none`}
    >
      <div
        className={`absolute transition-colors ${
          isVert ? 'inset-y-0 left-[2px] w-px' : 'inset-x-0 top-[2px] h-px'
        } ${dragging ? 'bg-[#378ef0]' : 'bg-[#3c3c3c] group-hover:bg-[#378ef0]'}`}
      />
    </div>
  );
}

