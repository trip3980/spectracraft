import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Activity,
  AudioWaveform,
  BarChart3,
  Box,
  BoxSelect,
  Crosshair,
  Eye,
  FileText,
  FolderOpen,
  MousePointer2,
  Pause,
  Play,
  Search,
  SkipBack,
  SlidersHorizontal,
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
import { HelpModal, type HelpModalTab } from './components/HelpModal.tsx';
import { MenuBar, type TopMenuDefinition } from './components/MenuBar.tsx';
import { ResynthesisLineageModal } from './components/ResynthesisLineageModal.tsx';
import { RightTabbedPanel } from './components/RightTabbedPanel.tsx';
import { SettingsModal, type SettingsFocusSection } from './components/SettingsModal.tsx';
import { StudioBottomDock, type BottomDockTab } from './components/StudioBottomDock.tsx';
import { StudioInspector, type InspectorTab } from './components/StudioInspector.tsx';
import { StudioPlanCanvas } from './components/StudioPlanCanvas.tsx';
import { TechnicalReportModal } from './components/TechnicalReportModal.tsx';
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
import type {
  ReportExportResponse,
  TechnicalReport,
  TechnicalReportApiResponse,
} from './lib/technicalReport.ts';
import type { EmbeddedDocMeta, HelpNavigationAction } from './lib/helpCatalog.ts';
import {
  DEFAULT_SPECTRACRAFT_SETTINGS,
  DEFAULT_WORKSPACE_PREFS,
  makeWorkspacePreset,
  pickRecommendedUpdateTarget,
  type RightPanelTab,
  type SettingsSnapshot,
  type SpectraCraftSettings,
  type UpdateActionKind,
  type UpdateActionResult,
  type UpdateCheckReport,
  type WorkspacePrefs,
} from './lib/settingsAndUpdates.ts';
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
  resizeBottomDockHeight,
  resizeSpatialWidth,
  savePersistedWorkspaceLayout,
  type WorkspaceLayoutState,
} from './lib/workspaceLayout.ts';

type MenuId = 'file' | 'edit' | 'view' | 'workspace' | 'process' | 'help' | null;

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
  const [layout, setLayout] = useState<WorkspaceLayoutState>(() => {
    const base = loadPersistedWorkspaceLayout();
    return {
      ...base,
      inspectorOpen: DEFAULT_WORKSPACE_PREFS.show_inspector,
      spatialOpen: DEFAULT_WORKSPACE_PREFS.show_room_views,
      bottomDockCollapsed: !DEFAULT_WORKSPACE_PREFS.show_diagnostics_dock,
    };
  });
  const [workspacePrefs, setWorkspacePrefs] = useState<WorkspacePrefs>(DEFAULT_WORKSPACE_PREFS);
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>('source');
  const [dockTab, setDockTab] = useState<BottomDockTab>('review');
  const [exportModalOpen, setExportModalOpen] = useState(false);
  const [lineageModalOpen, setLineageModalOpen] = useState(false);
  const [reportModalOpen, setReportModalOpen] = useState(false);
  const [technicalReport, setTechnicalReport] = useState<TechnicalReport | null>(null);
  const [technicalReportId, setTechnicalReportId] = useState<string | null>(null);
  const [reportBusy, setReportBusy] = useState(false);
  const [reportError, setReportError] = useState<string | null>(null);
  const [reportExportBusy, setReportExportBusy] = useState<'pdf' | 'markdown' | 'docx' | null>(null);
  const [reportLastExport, setReportLastExport] = useState<ReportExportResponse | null>(null);
  const [isDraggingFile, setIsDraggingFile] = useState(false);

  // Help system modal state
  const [helpModalOpen, setHelpModalOpen] = useState(false);
  const [helpInitialTab, setHelpInitialTab] = useState<HelpModalTab>('search');
  const [helpInitialQuery, setHelpInitialQuery] = useState('');
  const [helpInitialDocId, setHelpInitialDocId] = useState<EmbeddedDocMeta['id']>('readme');

  // ArtCraft settings (craft-settings) & quiet non-blocking background update check
  const [settingsSnapshot, setSettingsSnapshot] = useState<SettingsSnapshot | null>(null);
  const [settingsModalOpen, setSettingsModalOpen] = useState(false);
  const [settingsFocusSection, setSettingsFocusSection] = useState<SettingsFocusSection>(null);
  const [settingsSaving, setSettingsSaving] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [updateReport, setUpdateReport] = useState<UpdateCheckReport | null>(null);
  const [updateCheckBusy, setUpdateCheckBusy] = useState(false);
  const [updateActionBusy, setUpdateActionBusy] = useState<UpdateActionKind | null>(null);
  const [updateActionResult, setUpdateActionResult] = useState<UpdateActionResult | null>(null);

  const openSettings = useCallback((section: SettingsFocusSection = null) => {
    setOpenMenu(null);
    setSettingsFocusSection(section);
    setSettingsModalOpen(true);
  }, []);

  useEffect(() => {
    savePersistedWorkspaceLayout(layout);
  }, [layout]);

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
  const [layerMixPlaying, setLayerMixPlaying] = useState(false);
  const layerMixNodesRef = useRef<
    Partial<Record<LayerChannel['id'], { source: AudioBufferSourceNode; gain: GainNode }>>
  >({});

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

  // Real Web Audio multi-stem inspection mixer (obeys Mute / Solo / Gain / Include Texture live)
  const effectiveGains = useMemo(
    () => computeEffectiveLayerGains(layerChannels, 0.707, includeTexture),
    [layerChannels, includeTexture],
  );

  const stopLayerMix = useCallback(() => {
    const entries = Object.values(layerMixNodesRef.current);
    for (const entry of entries) {
      if (!entry) continue;
      try {
        entry.source.onended = null;
        entry.source.stop();
        entry.source.disconnect();
      } catch {
        // already stopped
      }
      try {
        entry.gain.disconnect();
      } catch {
        // already disconnected
      }
    }
    layerMixNodesRef.current = {};
    setLayerMixPlaying(false);
  }, []);

  const handleToggleLayerMixPlay = useCallback(() => {
    if (layerMixPlaying) {
      stopLayerMix();
      return;
    }
    const stemEntries = (
      Object.entries(layerBuffers) as Array<[LayerChannel['id'], AudioBuffer | undefined]>
    ).filter((pair): pair is [LayerChannel['id'], AudioBuffer] => Boolean(pair[1]));
    if (stemEntries.length === 0) return;

    stopLayerMix();
    const ctx = ensureAudioContext();
    if (ctx.state === 'suspended') {
      void ctx.resume();
    }
    const startAt = ctx.currentTime + 0.02;
    const nextNodes: Partial<
      Record<LayerChannel['id'], { source: AudioBufferSourceNode; gain: GainNode }>
    > = {};
    let longestId: LayerChannel['id'] = stemEntries[0][0];
    let longestDur = -1;
    for (const [id, buf] of stemEntries) {
      if (buf.duration > longestDur) {
        longestDur = buf.duration;
        longestId = id;
      }
    }

    for (const [id, buf] of stemEntries) {
      const source = ctx.createBufferSource();
      const gain = ctx.createGain();
      source.buffer = buf;
      gain.gain.setValueAtTime(effectiveGains[id] ?? 0, startAt);
      source.connect(gain);
      gain.connect(ctx.destination);
      if (id === longestId) {
        source.onended = () => {
          if (layerMixNodesRef.current[id]?.source === source) {
            layerMixNodesRef.current = {};
            setLayerMixPlaying(false);
          }
        };
      }
      source.start(startAt);
      nextNodes[id] = { source, gain };
    }
    layerMixNodesRef.current = nextNodes;
    setLayerMixPlaying(true);
  }, [layerMixPlaying, layerBuffers, stopLayerMix, ensureAudioContext, effectiveGains]);

  useEffect(() => {
    if (!layerMixPlaying) return;
    const ctx = ensureAudioContext();
    const now = ctx.currentTime;
    for (const [id, node] of Object.entries(layerMixNodesRef.current)) {
      if (!node) continue;
      const target = effectiveGains[id] ?? 0;
      try {
        node.gain.gain.cancelScheduledValues(now);
        node.gain.gain.setValueAtTime(node.gain.gain.value, now);
        node.gain.gain.linearRampToValueAtTime(target, now + 0.02);
      } catch {
        // ignore if audio context is closing
      }
    }
  }, [layerMixPlaying, effectiveGains, ensureAudioContext]);

  useEffect(() => () => stopLayerMix(), [stopLayerMix]);

  // Fetch local capture-reference studio catalogs once on mount
  useEffect(() => {
    let active = true;
    fetch('/api/studio/catalogs')
      .then(async (res) => {
        const body = (await res.json().catch(() => null)) as
          | (RustStudioCatalogs & { error?: string })
          | null;
        if (!res.ok) {
          throw new Error(body?.error || `HTTP ${res.status}`);
        }
        if (!body) {
          throw new Error('Empty catalog response');
        }
        return body;
      })
      .then((data: RustStudioCatalogs) => {
        if (active) setCatalogs(data);
      })
      .catch((err) => {
        if (active) {
          setStatusTone('error');
          setStatusText(
            `Catalog load failed: ${err instanceof Error ? err.message : String(err)}`,
          );
        }
      });
    return () => {
      active = false;
    };
  }, []);

  const handleCheckUpdates = useCallback(async (online: boolean) => {
    setUpdateCheckBusy(true);
    try {
      const res = await fetch(`/api/updates/check?online=${online ? '1' : '0'}`);
      if (!res.ok) return;
      const data = (await res.json()) as UpdateCheckReport;
      setUpdateReport(data);
    } catch {
      // Background update check must never block or fail the workstation
    } finally {
      setUpdateCheckBusy(false);
    }
  }, []);

  const applyWorkspacePrefsToUi = useCallback((ws: WorkspacePrefs) => {
    setWorkspacePrefs(ws);
    setLayout((prev) => ({
      ...prev,
      inspectorOpen: ws.show_inspector,
      spatialOpen: ws.show_room_views,
      spatialMode: ws.spatial_mode,
      bottomDockCollapsed: !ws.show_diagnostics_dock,
    }));
    if (ws.show_waveform && ws.show_spectrogram) {
      setDisplayMode('both');
    } else if (ws.show_waveform) {
      setDisplayMode('waveform');
    } else if (ws.show_spectrogram) {
      setDisplayMode('spectrogram');
    }
  }, []);

  const persistWorkspacePatch = useCallback(
    (patch: Partial<WorkspacePrefs>, resetGeometry = false) => {
      const nextWs: WorkspacePrefs = { ...workspacePrefs, ...patch };
      applyWorkspacePrefsToUi(nextWs);
      if (resetGeometry) {
        const def = createDefaultWorkspaceLayout();
        setLayout({
          ...def,
          inspectorOpen: nextWs.show_inspector,
          spatialOpen: nextWs.show_room_views,
          spatialMode: nextWs.spatial_mode,
          bottomDockCollapsed: !nextWs.show_diagnostics_dock,
        });
      }
      const baseSettings = settingsSnapshot?.settings ?? DEFAULT_SPECTRACRAFT_SETTINGS;
      const nextSettings: SpectraCraftSettings = {
        ...baseSettings,
        workspace: nextWs,
      };
      setSettingsSnapshot((prev) => (prev ? { ...prev, settings: nextSettings } : prev));
      fetch('/api/settings', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(nextSettings),
      })
        .then((res) => (res.ok ? res.json() : null))
        .then((snap: SettingsSnapshot | null) => {
          if (snap) setSettingsSnapshot(snap);
        })
        .catch(() => {
          // Non-blocking persistence; local UI state already reflects user choice
        });
    },
    [workspacePrefs, settingsSnapshot, applyWorkspacePrefsToUi],
  );

  const openRightPanelTab = useCallback(
    (tab: RightPanelTab) => {
      persistWorkspacePatch({
        show_right_panel: true,
        right_panel_tab: tab,
        ...(tab === 'inspector' ? { show_inspector: true } : {}),
        ...(tab === 'room' ? { show_room_views: true } : {}),
      });
    },
    [persistWorkspacePatch],
  );

  // Load persisted ArtCraft craft-settings on launch and trigger optional non-blocking update check
  useEffect(() => {
    let active = true;
    fetch('/api/settings')
      .then((res) => (res.ok ? res.json() : null))
      .then((snap: SettingsSnapshot | null) => {
        if (!active || !snap) return;
        setSettingsSnapshot(snap);
        if (snap.settings.workspace) {
          applyWorkspacePrefsToUi(snap.settings.workspace);
        }
        if (snap.settings.updates.check_on_launch) {
          void handleCheckUpdates(true);
        }
      })
      .catch(() => {
        // Keep documented defaults if local settings check is temporarily unavailable
      });
    return () => {
      active = false;
    };
  }, [handleCheckUpdates, applyWorkspacePrefsToUi]);

  const handleSaveSettings = useCallback(async (next: SpectraCraftSettings): Promise<boolean> => {
    setSettingsSaving(true);
    setSettingsError(null);
    try {
      const res = await fetch('/api/settings', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(next),
      });
      const data = (await res.json()) as SettingsSnapshot & { error?: string };
      if (!res.ok) throw new Error(data.error || 'Failed to persist settings');
      setSettingsSnapshot(data);
      if (data.settings.workspace) {
        applyWorkspacePrefsToUi(data.settings.workspace);
      }
      setStatusTone('ready');
      setStatusText('Saved SpectraCraft settings to local craft-settings config.toml');
      return true;
    } catch (err) {
      setSettingsError(err instanceof Error ? err.message : String(err));
      return false;
    } finally {
      setSettingsSaving(false);
    }
  }, [applyWorkspacePrefsToUi]);

  const handleResetSettings = useCallback(async (): Promise<boolean> => {
    setSettingsSaving(true);
    setSettingsError(null);
    try {
      const res = await fetch('/api/settings/reset', { method: 'POST' });
      const data = (await res.json()) as SettingsSnapshot & { error?: string };
      if (!res.ok) throw new Error(data.error || 'Failed to reset settings');
      setSettingsSnapshot(data);
      if (data.settings.workspace) {
        applyWorkspacePrefsToUi(data.settings.workspace);
      }
      setStatusTone('ready');
      setStatusText('Restored SpectraCraft ArtCraft settings to defaults');
      return true;
    } catch (err) {
      setSettingsError(err instanceof Error ? err.message : String(err));
      return false;
    } finally {
      setSettingsSaving(false);
    }
  }, [applyWorkspacePrefsToUi]);

  const handleUpdateAction = useCallback(
    async (action: UpdateActionKind) => {
      const noticeId = updateReport?.latest_notice_id ?? '';
      const target = pickRecommendedUpdateTarget(updateReport) ?? '';
      setUpdateActionBusy(action);
      setUpdateActionResult(null);
      try {
        const res = await fetch('/api/updates/action', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ action, noticeId, target }),
        });
        const result = (await res.json()) as UpdateActionResult & { error?: string };
        if (!res.ok) throw new Error(result.error || `Update action '${action}' failed`);
        setUpdateActionResult(result);
        // Refresh persisted settings snapshot so last_action / pending_staged_target / dismissed_notice_id stay synced
        const snapRes = await fetch('/api/settings');
        if (snapRes.ok) {
          setSettingsSnapshot((await snapRes.json()) as SettingsSnapshot);
        }
        if (action === 'download_later' || action === 'update_now') {
          void handleCheckUpdates(false);
        }
        setStatusTone(result.ok ? 'ready' : 'error');
        setStatusText(result.message);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setUpdateActionResult({
          action,
          ok: false,
          message: msg,
          sweep_test_ran: false,
          sweep_test_passed: false,
          unit_tests_passed: false,
          rolled_back: false,
          staged_target: null,
          dismissed_notice_id: null,
        });
        setStatusTone('error');
        setStatusText(msg);
      } finally {
        setUpdateActionBusy(null);
      }
    },
    [updateReport, handleCheckUpdates],
  );

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
      stopLayerMix();
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
      setOrigBuffer(null);
      setRegenBuffer(null);
      setDiffBuffer(null);
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
      setPipelineResult(null);
      setRegionHistory(createInitialDryRegionHistory());
      setRegionEditResult(null);
      setRegionBeforeBuf(null);
      setRegionAfterBuf(null);
      setDryChecklist(emptyDryListeningChecklist());
      setLivePreview(false);
      setTechnicalReport(null);
      setTechnicalReportId(null);
      setReportError(null);
      setReportLastExport(null);
      await hydrateSessionAudio(payload);
      setStatusTone('ready');
      setStatusText(`Loaded ${payload.label} · Stage 1 dry take ready (post-dry stages OFF by default)`);
    },
    [hydrateSessionAudio, resetTransport, stopLayerMix],
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
    stopLayerMix();
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
    setSession(null);
    setOrigBuffer(null);
    setRegenBuffer(null);
    setCapturedBuffer(null);
    setPipelineResult(null);
    setDiffBuffer(null);
    setActiveDryUrl(null);
    setActiveCuesUrl(null);
    setActiveCuesRunId(null);
    setLayerSession(null);
    setLayerBuffers({});
    setViewedLayerId(null);
    setSelection(null);
    setSourceRegenResult(null);
    setRegionHistory(createInitialDryRegionHistory());
    setRegionEditResult(null);
    setRegionBeforeBuf(null);
    setRegionAfterBuf(null);
    setDryChecklist(emptyDryListeningChecklist());
    setLivePreview(false);
    setTechnicalReport(null);
    setTechnicalReportId(null);
    setReportError(null);
    setReportLastExport(null);
    setReportModalOpen(false);
    setStatusTone('idle');
    setStatusText('Ready');
  }, [resetTransport, stopLayerMix]);

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
    stopLayerMix();
    setLayersBusy(true);
    setStatusTone('busy');
    setStatusText('Running Rust analyze-layers & render-layers…');
    try {
      const startS = selection ? selection[0] : Math.min(20, Math.max(0, durationS - 7));
      const endS = selection ? selection[1] : Math.min(durationS, startS + 7);
      const res = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/layers`, {
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
  }, [session, selection, durationS, decodeWavUrl, stopLayerMix]);

  const handleRenderEditedCues = useCallback(async () => {
    if (!session) return;
    setCuesBusy(true);
    setStatusTone('busy');
    setStatusText('Running source-free Rust render-dry-guitar…');
    try {
      const patches = Object.values(cuePatches);
      const res = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/render-cues`, {
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
      setPipelineResult(null);
      setRegionEditResult(null);
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
      const res = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/regenerate-source`, {
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
        const res = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/dry-region-edit`, {
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
      openRightPanelTab('diagnostics');
      setDockTab('regions');
    },
    [durationS, openRightPanelTab],
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
      openRightPanelTab('inspector');
      setInspectorTab('source');
      setStatusTone('ready');
      setStatusText(
        `Applied rough character guess (${guess}${straightenRecommended ? ' + light straighten' : ''}) to Stage 1 Dry Voicing`,
      );
    },
    [openRightPanelTab],
  );

  const handleRunGapRepair = useCallback(
    async (startS: number, gapMs: number) => {
      if (!session) return;
      setRepairBusy(true);
      setStatusTone('busy');
      setStatusText('Running Rust repair-gap…');
      try {
        const res = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/repair-gap`, {
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
      const res = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/variants`, {
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
        const res = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/pipeline-render`, {
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
    openRightPanelTab('diagnostics');
    setDockTab('review');
  }, [openRightPanelTab]);

  const handleGenerateTechnicalReport = useCallback(async () => {
    if (!session) return;
    setOpenMenu(null);
    setReportBusy(true);
    setReportError(null);
    setReportLastExport(null);
    setStatusTone('busy');
    setStatusText('Running blind Before & After technical audit in Rust…');
    try {
      const stages: string[] = ['Stage 1 Dry'];
      if (pipelineStages.micEnabled) stages.push('Stage 3 Mic');
      if (pipelineStages.roomEnabled) stages.push('Stage 4 Room');
      if (pipelineStages.pluginSlot.enabled && pipelineStages.pluginSlot.pluginId !== 'none') {
        stages.push(`Stage 5 Plugin (${pipelineStages.pluginSlot.pluginId})`);
      }
      const res = await fetch(
        `/api/sessions/${encodeURIComponent(session.id)}/technical-report`,
        {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            cuesRunId: activeCuesRunId ?? '',
            pipelineRunId: pipelineResult?.runId ?? '',
            title: `${session.label} — Blind Before/After Technical Report`,
            pipelineLabel: pipelineResult
              ? stages.join(' -> ')
              : 'Stage 1 Dry Regeneration (post-dry stages bypassed)',
          }),
        },
      );
      const data = (await res.json()) as Partial<TechnicalReportApiResponse> & { error?: string };
      if (!res.ok || !data.report || !data.reportId) {
        throw new Error(data.error || 'Blind technical report generation failed.');
      }
      setTechnicalReport(data.report);
      setTechnicalReportId(data.reportId);
      if (settingsSnapshot?.settings.report.auto_open_viewer ?? true) {
        openRightPanelTab('report');
      }
      setStatusTone('ready');
      setStatusText(
        `Blind technical report ready (${data.report.comparison.deltas.length} significant deltas)`,
      );
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setReportError(msg);
      openRightPanelTab('report');
      setStatusTone('error');
      setStatusText(msg);
    } finally {
      setReportBusy(false);
    }
  }, [session, activeCuesRunId, pipelineStages, pipelineResult, settingsSnapshot, openRightPanelTab]);

  const handleExportTechnicalReport = useCallback(
    async (format: 'pdf' | 'markdown' | 'docx'): Promise<ReportExportResponse | null> => {
      if (!session || !technicalReportId) return null;
      setReportExportBusy(format);
      setReportError(null);
      try {
        const res = await fetch(
          `/api/sessions/${encodeURIComponent(session.id)}/technical-report/export`,
          {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              reportId: technicalReportId,
              format,
            }),
          },
        );
        const data = (await res.json()) as Partial<ReportExportResponse> & { error?: string };
        if (!res.ok || !data.url || !data.receipt) {
          throw new Error(data.error || `Failed to export report as ${format.toUpperCase()}.`);
        }
        const exportResp = data as ReportExportResponse;
        setReportLastExport(exportResp);
        setStatusTone('ready');
        setStatusText(
          `Exported technical report (${exportResp.receipt.actual_format.toUpperCase()} via ${exportResp.receipt.engine})`,
        );
        return exportResp;
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setReportError(msg);
        setStatusTone('error');
        setStatusText(msg);
        return null;
      } finally {
        setReportExportBusy(null);
      }
    },
    [session, technicalReportId],
  );

  const openHelp = useCallback(
    (tab: HelpModalTab, opts?: { query?: string; docId?: EmbeddedDocMeta['id'] }) => {
      setOpenMenu(null);
      setHelpInitialTab(tab);
      setHelpInitialQuery(opts?.query ?? '');
      if (opts?.docId) setHelpInitialDocId(opts.docId);
      setHelpModalOpen(true);
    },
    [],
  );

  const handleHelpNavigate = useCallback(
    (action: HelpNavigationAction) => {
      switch (action) {
        case 'open-wav':
          fileInputRef.current?.click();
          break;
        case 'open-reference':
          void handleOpenReference();
          break;
        case 'export-modal':
          if (session) setExportModalOpen(true);
          else {
            setStatusTone('idle');
            setStatusText('Open a WAV file first to export dry audio, cues, or variants.');
          }
          break;
        case 'stage1-source':
          openRightPanelTab('inspector');
          setInspectorTab('source');
          break;
        case 'stage1-shape':
          openRightPanelTab('diagnostics');
          setDockTab('shape');
          break;
        case 'stage2-regions':
          setActiveTool('marquee');
          openRightPanelTab('diagnostics');
          setDockTab('regions');
          break;
        case 'stage3-mic':
          openRightPanelTab('inspector');
          setInspectorTab('mic');
          break;
        case 'stage4-room':
          openRightPanelTab('inspector');
          setInspectorTab('room');
          break;
        case 'stage4-materials':
          openRightPanelTab('inspector');
          setInspectorTab('materials');
          break;
        case 'stage5-plugin':
          openRightPanelTab('inspector');
          setInspectorTab('source');
          break;
        case 'layers-dock':
          openRightPanelTab('diagnostics');
          setDockTab('layers');
          break;
        case 'cues-dock':
          openRightPanelTab('diagnostics');
          setDockTab('cues');
          break;
        case 'repair-dock':
          openRightPanelTab('diagnostics');
          setDockTab('repair');
          break;
        case 'dry-gate-dock':
          openRightPanelTab('diagnostics');
          setDockTab('review');
          break;
        case 'technical-report':
        case 'report-export-pdf':
          openRightPanelTab('report');
          if (!technicalReport && session) {
            void handleGenerateTechnicalReport();
          } else if (!session && !technicalReport) {
            setStatusTone('idle');
            setStatusText('Open a WAV or Reference session first to generate a blind Before/After Technical Report.');
          }
          break;
        case 'workspace-waveform':
          persistWorkspacePatch({ show_waveform: true });
          break;
        case 'workspace-spectrogram':
          persistWorkspacePatch({ show_spectrogram: true });
          break;
        case 'workspace-transport':
          persistWorkspacePatch({ show_transport: true });
          break;
        case 'workspace-room-views':
          openRightPanelTab('room');
          break;
        case 'workspace-tool-rail':
          persistWorkspacePatch({ show_tool_rail: true });
          break;
        case 'workspace-right-panel':
          persistWorkspacePatch({ show_right_panel: true });
          break;
        case 'workspace-reset':
          persistWorkspacePatch(DEFAULT_WORKSPACE_PREFS, true);
          break;
        case 'lineage-modal':
          setLineageModalOpen(true);
          break;
        case 'settings-modal':
          openSettings(null);
          break;
        case 'settings-appearance':
          openSettings('appearance');
          break;
        case 'settings-workspace':
          openSettings('workspace');
          break;
        case 'settings-updates':
          openSettings('updates');
          void handleCheckUpdates(true);
          break;
        case 'settings-paths':
          openSettings('paths');
          break;
        case 'doc-readme':
          openHelp('docs', { docId: 'readme' });
          break;
        case 'doc-spec':
          openHelp('docs', { docId: 'spec' });
          break;
        case 'doc-craft-libs':
          openHelp('docs', { docId: 'craft-libs' });
          break;
      }
    },
    [
      handleOpenReference,
      session,
      persistWorkspacePatch,
      openRightPanelTab,
      technicalReport,
      handleGenerateTechnicalReport,
      handleCheckUpdates,
      openSettings,
      openHelp,
    ],
  );

  // Global keyboard shortcuts (Space, Home, 1-4 takes, V/M/R/Z tools, Undo, Redo, Open, Export, Settings, Help)
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement | null)?.tagName;
      const inInput = tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
      if (e.key === 'Escape') {
        setOpenMenu(null);
        setExportModalOpen(false);
        setLineageModalOpen(false);
        setReportModalOpen(false);
        setSettingsModalOpen(false);
        setSettingsFocusSection(null);
        setHelpModalOpen(false);
      } else if (e.key === 'F1' || ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k')) {
        e.preventDefault();
        openHelp('search');
      } else if ((e.metaKey || e.ctrlKey) && e.key === ',') {
        e.preventDefault();
        openSettings(null);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'e' && session) {
        e.preventDefault();
        setExportModalOpen(true);
      } else if (e.code === 'Space' && !inInput && session) {
        e.preventDefault();
        handleTogglePlay();
      } else if (e.key === 'Home' && !inInput && session) {
        e.preventDefault();
        handleSeek(0);
      } else if (!inInput && !e.metaKey && !e.ctrlKey && !e.altKey) {
        const lower = e.key.toLowerCase();
        if (lower === 'v') setActiveTool('scrub');
        else if (lower === 'm') setActiveTool('marquee');
        else if (lower === 'r') setActiveTool('range');
        else if (lower === 'z') setActiveTool('zoom');
        else if (session && e.key === '1') handleSwitchTake('original');
        else if (session && e.key === '2') handleSwitchTake('regenerated');
        else if (session && e.key === '3' && capturedBuffer) handleSwitchTake('captured');
        else if (session && e.key === '4') handleSwitchTake('difference');
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'z') {
        e.preventDefault();
        if (e.shiftKey) handleRedo();
        else handleUndo();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'o') {
        e.preventDefault();
        fileInputRef.current?.click();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [
    session,
    capturedBuffer,
    handleTogglePlay,
    handleSeek,
    handleSwitchTake,
    handleUndo,
    handleRedo,
    openHelp,
    openSettings,
  ]);

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
  const capturedSha = pipelineResult?.wav_sha256 ?? null;
  const resultTrackLabel =
    takeMode === 'difference'
      ? 'Difference (Regenerated − Original)'
      : takeMode === 'captured' && capturedBuffer
        ? pipelineResult?.summary.bit_identical_to_dry
          ? `Pipeline Take (All Post-Dry Bypassed · Bit-Identical to Dry · SHA ${capturedSha ? capturedSha.slice(0, 8) : '—'})`
          : `Captured Take (Rust 5-Stage Pipeline · SHA ${capturedSha ? capturedSha.slice(0, 8) : '—'})`
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

  const rightMeta = session
    ? `${session.label} · ${(sampleRate / 1000).toFixed(1)} kHz mono · ${durationS.toFixed(3)} s`
    : 'No file open';

  const playheadReadout = session
    ? `${isPlaying ? 'Playing' : 'Stopped'} · ${playheadS.toFixed(2)}s / ${durationS.toFixed(2)}s`
    : null;

  const menus: TopMenuDefinition[] = useMemo(
    () => [
      {
        id: 'file',
        label: 'File',
        items: [
          {
            kind: 'item',
            id: 'file-open-wav',
            label: 'Open Single-Source WAV…',
            shortcut: 'Ctrl+O',
            onSelect: () => fileInputRef.current?.click(),
          },
          {
            kind: 'item',
            id: 'file-open-reference',
            label: 'Open Slow Solo Reference (48 kHz)',
            onSelect: () => void handleOpenReference(),
          },
          { kind: 'separator' },
          {
            kind: 'item',
            id: 'file-export',
            label: 'Export Dry WAV / Cues / Variants…',
            shortcut: 'Ctrl+E',
            disabled: !session,
            onSelect: () => setExportModalOpen(true),
          },
          {
            kind: 'item',
            id: 'file-generate-report',
            label: reportBusy
              ? 'Generating Before/After Technical Report…'
              : 'Generate Blind Before/After Technical Report…',
            disabled: !session || reportBusy,
            onSelect: () => void handleGenerateTechnicalReport(),
          },
          {
            kind: 'item',
            id: 'file-open-report',
            label: 'Open Technical Report Tab',
            disabled: !session && !technicalReport,
            onSelect: () => {
              openRightPanelTab('report');
              if (!technicalReport && session) void handleGenerateTechnicalReport();
            },
          },
          { kind: 'separator' },
          {
            kind: 'item',
            id: 'file-settings',
            label: 'Settings & Preferences…',
            shortcut: 'Ctrl+,',
            onSelect: () => openSettings(null),
          },
          { kind: 'separator' },
          {
            kind: 'item',
            id: 'file-close',
            label: 'Close File',
            disabled: !session,
            onSelect: handleCloseFile,
          },
        ],
      },
      {
        id: 'edit',
        label: 'Edit',
        items: [
          {
            kind: 'item',
            id: 'edit-undo',
            label: 'Undo',
            shortcut: 'Ctrl+Z',
            disabled: rigHistory.past.length === 0,
            onSelect: handleUndo,
          },
          {
            kind: 'item',
            id: 'edit-redo',
            label: 'Redo',
            shortcut: 'Ctrl+Shift+Z',
            disabled: rigHistory.future.length === 0,
            onSelect: handleRedo,
          },
          { kind: 'separator' },
          {
            kind: 'submenu',
            id: 'edit-canvas-tool',
            label: 'Canvas Tool',
            items: [
              {
                kind: 'item',
                id: 'tool-scrub',
                label: 'Pointer / Scrub',
                shortcut: 'V',
                checked: activeTool === 'scrub',
                onSelect: () => setActiveTool('scrub'),
              },
              {
                kind: 'item',
                id: 'tool-marquee',
                label: 'Rectangular Marquee (Stage 2 Dry)',
                shortcut: 'M',
                checked: activeTool === 'marquee',
                onSelect: () => setActiveTool('marquee'),
              },
              {
                kind: 'item',
                id: 'tool-range',
                label: 'In / Out Range Selection (Stage 2 Dry)',
                shortcut: 'R',
                checked: activeTool === 'range',
                onSelect: () => setActiveTool('range'),
              },
              {
                kind: 'item',
                id: 'tool-zoom',
                label: 'Zoom Tool (Click In / Shift+Click Out)',
                shortcut: 'Z',
                checked: activeTool === 'zoom',
                onSelect: () => setActiveTool('zoom'),
              },
            ],
          },
          {
            kind: 'submenu',
            id: 'edit-dry-regions',
            label: `Stage 2 Dry Regions (${regionHistory.regions.length})`,
            items: [
              {
                kind: 'item',
                id: 'region-quick-add',
                label: 'Add Dry Region at Playhead / Selection',
                disabled: !session,
                onSelect: () => {
                  openRightPanelTab('diagnostics');
                  setDockTab('regions');
                  handleQuickAddDryRegion('marquee');
                },
              },
              {
                kind: 'item',
                id: 'region-undo',
                label: 'Undo Dry Region Edit',
                disabled: regionHistory.past.length === 0,
                onSelect: () => setRegionHistory((prev) => undoDryRegionHistory(prev)),
              },
              {
                kind: 'item',
                id: 'region-redo',
                label: 'Redo Dry Region Edit',
                disabled: regionHistory.future.length === 0,
                onSelect: () => setRegionHistory((prev) => redoDryRegionHistory(prev)),
              },
              { kind: 'separator' },
              {
                kind: 'item',
                id: 'region-open-dock',
                label: 'Open Surgical Region Edits Tab',
                onSelect: () => {
                  openRightPanelTab('diagnostics');
                  setDockTab('regions');
                },
              },
            ],
          },
          { kind: 'separator' },
          {
            kind: 'item',
            id: 'edit-clear-selection',
            label: 'Clear Time Selection',
            disabled: !selection,
            onSelect: () => setSelection(null),
          },
          {
            kind: 'item',
            id: 'edit-reset-rig',
            label: 'Reset Room & Rig (Small Room)',
            onSelect: () => handleUpdateRig(makeRoomPreset('small'), true),
          },
        ],
      },
      {
        id: 'view',
        label: 'View',
        items: [
          {
            kind: 'submenu',
            id: 'view-transport',
            label: 'Playback & Take A/B',
            items: [
              {
                kind: 'item',
                id: 'play-toggle',
                label: isPlaying ? 'Pause Playback' : 'Play Audio',
                shortcut: 'Space',
                disabled: !session || loadingAudio,
                onSelect: handleTogglePlay,
              },
              {
                kind: 'item',
                id: 'play-rewind',
                label: 'Rewind to Start',
                shortcut: 'Home',
                disabled: !session || loadingAudio,
                onSelect: () => handleSeek(0),
              },
              { kind: 'separator' },
              {
                kind: 'item',
                id: 'take-original',
                label: 'Original (Reference Input)',
                shortcut: '1',
                checked: takeMode === 'original',
                disabled: !session,
                onSelect: () => handleSwitchTake('original'),
              },
              {
                kind: 'item',
                id: 'take-dry',
                label: 'Dry Take (Stage 1 / 2)',
                shortcut: '2',
                checked: takeMode === 'regenerated',
                disabled: !session,
                onSelect: () => handleSwitchTake('regenerated'),
              },
              {
                kind: 'item',
                id: 'take-captured',
                label: 'Captured Take (Stages 3–5)',
                shortcut: '3',
                checked: takeMode === 'captured',
                disabled: !session || !capturedBuffer,
                onSelect: () => handleSwitchTake('captured'),
              },
              {
                kind: 'item',
                id: 'take-difference',
                label: 'Difference (Regenerated − Original)',
                shortcut: '4',
                checked: takeMode === 'difference',
                disabled: !session,
                onSelect: () => handleSwitchTake('difference'),
              },
              { kind: 'separator' },
              {
                kind: 'item',
                id: 'view-level-match',
                label: 'Level-Match A/B Playback (Match dB)',
                checked: levelMatchAb,
                disabled: !session,
                onSelect: () => setLevelMatchAb((v) => !v),
              },
            ],
          },
          {
            kind: 'submenu',
            id: 'view-display-mode',
            label: 'Audio Canvas Display',
            items: [
              {
                kind: 'item',
                id: 'display-both',
                label: 'Both (Spectrogram + Waveform)',
                checked: workspacePrefs.show_spectrogram && workspacePrefs.show_waveform,
                onSelect: () =>
                  persistWorkspacePatch({ show_spectrogram: true, show_waveform: true }),
              },
              {
                kind: 'item',
                id: 'display-spectrogram',
                label: 'Spectrogram Only',
                checked: workspacePrefs.show_spectrogram && !workspacePrefs.show_waveform,
                onSelect: () =>
                  persistWorkspacePatch({ show_spectrogram: true, show_waveform: false }),
              },
              {
                kind: 'item',
                id: 'display-waveform',
                label: 'Waveform Only',
                checked: !workspacePrefs.show_spectrogram && workspacePrefs.show_waveform,
                onSelect: () =>
                  persistWorkspacePatch({ show_spectrogram: false, show_waveform: true }),
              },
            ],
          },
          {
            kind: 'submenu',
            id: 'view-max-hz',
            label: `Spectrogram Max Frequency (${(displayMaxHz / 1000).toFixed(0)} kHz)`,
            items: [8000, 12000, 16000, 20000, 24000, 32000].map((hz) => ({
              kind: 'item' as const,
              id: `max-hz-${hz}`,
              label: `${hz / 1000} kHz`,
              checked: displayMaxHz === hz,
              onSelect: () => setDisplayMaxHz(hz),
            })),
          },
          {
            kind: 'submenu',
            id: 'view-db-floor',
            label: `Spectrogram Contrast Floor (${spectrogramDbFloor} dB)`,
            items: [
              { val: -78, label: '-78 dB (High Contrast)' },
              { val: -96, label: '-96 dB (Standard)' },
              { val: -112, label: '-112 dB (Deep Floor)' },
            ].map(({ val, label }) => ({
              kind: 'item' as const,
              id: `db-floor-${Math.abs(val)}`,
              label,
              checked: spectrogramDbFloor === val,
              onSelect: () => setSpectrogramDbFloor(val),
            })),
          },
          {
            kind: 'submenu',
            id: 'view-bit-depth',
            label: `Preview Bit Depth (${previewBitDepth === 32 ? '32f' : `${previewBitDepth}b`})`,
            items: (
              [
                [32, '32-bit Float (32f)'],
                [24, '24-bit PCM (24b)'],
                [16, '16-bit PCM (16b)'],
              ] as [32 | 24 | 16, string][]
            ).map(([bits, label]) => ({
              kind: 'item' as const,
              id: `bit-depth-${bits}`,
              label,
              checked: previewBitDepth === bits,
              onSelect: () => setPreviewBitDepth(bits),
            })),
          },
          { kind: 'separator' },
          {
            kind: 'item',
            id: 'view-evidence',
            label: 'Cue Evidence Overlay (Visual Only)',
            checked: showEvidenceOverlay,
            disabled: !session,
            onSelect: () => setShowEvidenceOverlay((v) => !v),
          },
          {
            kind: 'submenu',
            id: 'view-zoom',
            label: 'Zoom & Navigation',
            items: [
              {
                kind: 'item',
                id: 'zoom-in',
                label: 'Zoom In (2×)',
                disabled: !session,
                onSelect: () => zoomBy(0.5),
              },
              {
                kind: 'item',
                id: 'zoom-out',
                label: 'Zoom Out (0.5×)',
                disabled: !session,
                onSelect: () => zoomBy(2.0),
              },
              {
                kind: 'item',
                id: 'zoom-fit',
                label: 'Fit Full Recording',
                disabled: !session,
                onSelect: () => {
                  setViewStartS(0);
                  setViewEndS(durationS);
                },
              },
              {
                kind: 'item',
                id: 'zoom-selection',
                label: 'Zoom to Selection',
                disabled: !selection,
                onSelect: () => {
                  if (selection) {
                    setViewStartS(selection[0]);
                    setViewEndS(selection[1]);
                  }
                },
              },
              { kind: 'separator' },
              {
                kind: 'item',
                id: 'jump-2550',
                label: 'Jump to 25.50 s Transition Window',
                disabled: durationS < 26,
                onSelect: () => {
                  handleSeek(25.5);
                  setViewStartS(24.5);
                  setViewEndS(Math.min(durationS, 26.5));
                },
              },
            ],
          },
          { kind: 'separator' },
          {
            kind: 'item',
            id: 'view-report-viewer',
            label: 'Technical Report Viewer…',
            disabled: !session && !technicalReport,
            onSelect: () => {
              if (technicalReport) setReportModalOpen(true);
              else if (session) void handleGenerateTechnicalReport();
            },
          },
          {
            kind: 'item',
            id: 'view-lineage',
            label: 'Resynthesis Lineage & Scope…',
            onSelect: () => setLineageModalOpen(true),
          },
        ],
      },
      {
        id: 'workspace',
        label: 'Workspace',
        items: [
          { kind: 'section', label: 'ARTCRAFT PANELS & PALETTE (REMEMBERED)' },
          {
            kind: 'item',
            id: 'ws-tool-palette',
            label: 'Left Tool Palette',
            checked: workspacePrefs.show_tool_rail,
            onSelect: () =>
              persistWorkspacePatch({ show_tool_rail: !workspacePrefs.show_tool_rail }),
          },
          {
            kind: 'item',
            id: 'ws-right-panel',
            label: 'Right Tabbed Panel',
            checked: workspacePrefs.show_right_panel,
            onSelect: () =>
              persistWorkspacePatch({ show_right_panel: !workspacePrefs.show_right_panel }),
          },
          {
            kind: 'submenu',
            id: 'ws-right-panel-tab',
            label: 'Right Panel Tab',
            items: [
              {
                kind: 'item',
                id: 'ws-right-tab-inspector',
                label: 'Inspector (Stages 1, 3–5)',
                checked:
                  workspacePrefs.show_right_panel &&
                  workspacePrefs.right_panel_tab === 'inspector',
                onSelect: () => openRightPanelTab('inspector'),
              },
              {
                kind: 'item',
                id: 'ws-right-tab-diagnostics',
                label: 'Diagnostics & Review',
                checked:
                  workspacePrefs.show_right_panel &&
                  workspacePrefs.right_panel_tab === 'diagnostics',
                onSelect: () => openRightPanelTab('diagnostics'),
              },
              {
                kind: 'item',
                id: 'ws-right-tab-report',
                label: 'Before/After Report Viewer',
                checked:
                  workspacePrefs.show_right_panel && workspacePrefs.right_panel_tab === 'report',
                onSelect: () => openRightPanelTab('report'),
              },
              {
                kind: 'item',
                id: 'ws-right-tab-room',
                label: 'Room Views (Plans & 3D)',
                checked:
                  workspacePrefs.show_right_panel && workspacePrefs.right_panel_tab === 'room',
                onSelect: () => openRightPanelTab('room'),
              },
            ],
          },
          { kind: 'separator' },
          {
            kind: 'item',
            id: 'ws-waveform',
            label: 'Waveform View',
            checked: workspacePrefs.show_waveform,
            onSelect: () =>
              persistWorkspacePatch({ show_waveform: !workspacePrefs.show_waveform }),
          },
          {
            kind: 'item',
            id: 'ws-spectrogram',
            label: 'Spectrogram View',
            checked: workspacePrefs.show_spectrogram,
            onSelect: () =>
              persistWorkspacePatch({ show_spectrogram: !workspacePrefs.show_spectrogram }),
          },
          {
            kind: 'item',
            id: 'ws-transport',
            label: 'Inline Transport Bar',
            checked: workspacePrefs.show_transport,
            onSelect: () =>
              persistWorkspacePatch({ show_transport: !workspacePrefs.show_transport }),
          },
          {
            kind: 'item',
            id: 'ws-diagnostics-dock',
            label: 'Bottom Diagnostics Dock (Optional)',
            checked: workspacePrefs.show_diagnostics_dock,
            onSelect: () =>
              persistWorkspacePatch({
                show_diagnostics_dock: !workspacePrefs.show_diagnostics_dock,
              }),
          },
          { kind: 'separator' },
          {
            kind: 'submenu',
            id: 'ws-room-mode',
            label: 'Room View Mode',
            items: [
              {
                kind: 'item',
                id: 'ws-room-plans',
                label: 'Plans (Top + Front)',
                checked: workspacePrefs.spatial_mode === 'plans',
                onSelect: () =>
                  persistWorkspacePatch({
                    show_right_panel: true,
                    right_panel_tab: 'room',
                    show_room_views: true,
                    spatial_mode: 'plans',
                  }),
              },
              {
                kind: 'item',
                id: 'ws-room-3d',
                label: '3D Grid',
                checked: workspacePrefs.spatial_mode === '3d',
                onSelect: () =>
                  persistWorkspacePatch({
                    show_right_panel: true,
                    right_panel_tab: 'room',
                    show_room_views: true,
                    spatial_mode: '3d',
                  }),
              },
              {
                kind: 'item',
                id: 'ws-room-split',
                label: 'Split (Plans + 3D)',
                checked: workspacePrefs.spatial_mode === 'split',
                onSelect: () =>
                  persistWorkspacePatch({
                    show_right_panel: true,
                    right_panel_tab: 'room',
                    show_room_views: true,
                    spatial_mode: 'split',
                  }),
              },
            ],
          },
          {
            kind: 'submenu',
            id: 'ws-dock-tabs',
            label: 'Diagnostics Sub-Tabs',
            items: [
              {
                kind: 'item',
                id: 'ws-tab-review',
                label: 'Dry Listening Gate (5 Checks)',
                checked:
                  workspacePrefs.show_right_panel &&
                  workspacePrefs.right_panel_tab === 'diagnostics' &&
                  dockTab === 'review',
                onSelect: () => {
                  openRightPanelTab('diagnostics');
                  setDockTab('review');
                },
              },
              {
                kind: 'item',
                id: 'ws-tab-regions',
                label: 'Stage 2 Surgical Regions',
                checked:
                  workspacePrefs.show_right_panel &&
                  workspacePrefs.right_panel_tab === 'diagnostics' &&
                  dockTab === 'regions',
                onSelect: () => {
                  openRightPanelTab('diagnostics');
                  setDockTab('regions');
                },
              },
              {
                kind: 'item',
                id: 'ws-tab-layers',
                label: 'Inspection Layers',
                checked:
                  workspacePrefs.show_right_panel &&
                  workspacePrefs.right_panel_tab === 'diagnostics' &&
                  dockTab === 'layers',
                onSelect: () => {
                  openRightPanelTab('diagnostics');
                  setDockTab('layers');
                },
              },
              {
                kind: 'item',
                id: 'ws-tab-cues',
                label: 'Performance Cues',
                checked:
                  workspacePrefs.show_right_panel &&
                  workspacePrefs.right_panel_tab === 'diagnostics' &&
                  dockTab === 'cues',
                onSelect: () => {
                  openRightPanelTab('diagnostics');
                  setDockTab('cues');
                },
              },
              {
                kind: 'item',
                id: 'ws-tab-repair',
                label: 'Local Gap Repair',
                checked:
                  workspacePrefs.show_right_panel &&
                  workspacePrefs.right_panel_tab === 'diagnostics' &&
                  dockTab === 'repair',
                onSelect: () => {
                  openRightPanelTab('diagnostics');
                  setDockTab('repair');
                },
              },
              {
                kind: 'item',
                id: 'ws-tab-shape',
                label: 'Reference Shape',
                checked:
                  workspacePrefs.show_right_panel &&
                  workspacePrefs.right_panel_tab === 'diagnostics' &&
                  dockTab === 'shape',
                onSelect: () => {
                  openRightPanelTab('diagnostics');
                  setDockTab('shape');
                },
              },
            ],
          },
          { kind: 'separator' },
          {
            kind: 'submenu',
            id: 'ws-presets',
            label: 'Workspace Presets',
            items: [
              {
                kind: 'item',
                id: 'ws-preset-calm',
                label: 'Calm Default (Tool Palette + Canvas + Right Panel)',
                onSelect: () => persistWorkspacePatch(makeWorkspacePreset('calm'), true),
              },
              {
                kind: 'item',
                id: 'ws-preset-full',
                label: 'Full Studio (+ Inline Transport + Room Tab)',
                onSelect: () => persistWorkspacePatch(makeWorkspacePreset('full_studio'), true),
              },
              {
                kind: 'item',
                id: 'ws-preset-diag',
                label: 'Diagnostics Focus (Right Diagnostics + Transport)',
                onSelect: () => persistWorkspacePatch(makeWorkspacePreset('diagnostics'), true),
              },
              {
                kind: 'item',
                id: 'ws-preset-report-audit',
                label: 'Report & Blind Audit (Dark-Mode Report Viewer)',
                onSelect: () => persistWorkspacePatch(makeWorkspacePreset('report_audit'), true),
              },
              {
                kind: 'item',
                id: 'ws-preset-canvas',
                label: 'Canvas Focus (Hide Side Panels)',
                onSelect: () => persistWorkspacePatch(makeWorkspacePreset('canvas_focus'), true),
              },
            ],
          },
          { kind: 'separator' },
          {
            kind: 'item',
            id: 'ws-reset',
            label: 'Reset Workspace',
            onSelect: () => persistWorkspacePatch(DEFAULT_WORKSPACE_PREFS, true),
          },
        ],
      },
      {
        id: 'process',
        label: 'Process',
        items: [
          { kind: 'section', label: 'FIXED FIVE-STAGE PIPELINE' },
          {
            kind: 'submenu',
            id: 'proc-stage1',
            label: 'Stage 1 — Dry Regeneration',
            items: [
              {
                kind: 'item',
                id: 'proc-synth-cues',
                label: 'Synthesize Dry WAV From Cues',
                disabled: !session || cuesBusy,
                onSelect: () => void handleRenderEditedCues(),
              },
              {
                kind: 'item',
                id: 'proc-regen-source',
                label: 'Regenerate Dry With Source Character',
                disabled: !session || sourceRegenBusy,
                onSelect: () => void handleRegenerateDryWithSource(),
              },
              { kind: 'separator' },
              {
                kind: 'item',
                id: 'proc-open-source-inspector',
                label: 'Open Source & Body Inspector',
                onSelect: () => {
                  openRightPanelTab('inspector');
                  setInspectorTab('source');
                },
              },
              {
                kind: 'item',
                id: 'proc-dry-gate',
                label: 'Open Dry Listening Gate (5 Checks)',
                disabled: !session,
                onSelect: openDryReviewDock,
              },
            ],
          },
          {
            kind: 'submenu',
            id: 'proc-stage2',
            label: `Stage 2 — Surgical Region Edits (${regionHistory.regions.length})`,
            items: [
              {
                kind: 'item',
                id: 'proc-marquee',
                label: 'Activate Marquee Region Tool',
                shortcut: 'M',
                checked: activeTool === 'marquee',
                onSelect: () => setActiveTool('marquee'),
              },
              {
                kind: 'item',
                id: 'proc-add-region',
                label: 'Quick-Add Dry Region',
                disabled: !session,
                onSelect: () => {
                  openRightPanelTab('diagnostics');
                  setDockTab('regions');
                  handleQuickAddDryRegion('marquee');
                },
              },
              {
                kind: 'item',
                id: 'proc-regions-dock',
                label: 'Open Surgical Regions Tab',
                onSelect: () => {
                  openRightPanelTab('diagnostics');
                  setDockTab('regions');
                },
              },
            ],
          },
          {
            kind: 'submenu',
            id: 'proc-stage3',
            label: `Stage 3 — Optional Microphone (${pipelineStages.micEnabled ? 'ON' : 'OFF'})`,
            items: [
              {
                kind: 'item',
                id: 'proc-toggle-mic',
                label: 'Enable Stage 3 Microphone',
                checked: pipelineStages.micEnabled,
                onSelect: () =>
                  setPipelineStages((p) => ({ ...p, micEnabled: !p.micEnabled })),
              },
              {
                kind: 'item',
                id: 'proc-open-mic-inspector',
                label: 'Open Microphone Inspector',
                onSelect: () => {
                  openRightPanelTab('inspector');
                  setInspectorTab('mic');
                },
              },
            ],
          },
          {
            kind: 'submenu',
            id: 'proc-stage4',
            label: `Stage 4 — Optional Space / Room (${pipelineStages.roomEnabled ? 'ON' : 'OFF'})`,
            items: [
              {
                kind: 'item',
                id: 'proc-toggle-room',
                label: 'Enable Stage 4 Space / Room',
                checked: pipelineStages.roomEnabled,
                onSelect: () =>
                  setPipelineStages((p) => ({ ...p, roomEnabled: !p.roomEnabled })),
              },
              {
                kind: 'item',
                id: 'proc-open-room-inspector',
                label: 'Open Room & Acoustics Inspector',
                onSelect: () => {
                  openRightPanelTab('inspector');
                  setInspectorTab('room');
                },
              },
              { kind: 'separator' },
              { kind: 'section', label: 'ROOM PRESETS' },
              ...(
                [
                  ['small', 'Small Room (10 × 10 × 10 ft)'],
                  ['medium', 'Medium Room (20 × 20 × 10 ft)'],
                  ['large', 'Large Room (50 × 50 × 10 ft)'],
                  ['open', 'Open Field (Ground Only)'],
                  ['direct', 'Direct (No Room)'],
                ] satisfies [RoomPresetKey, string][]
              ).map(([preset, title]) => ({
                kind: 'item' as const,
                id: `room-preset-${preset}`,
                label: title,
                checked: rig.preset === preset,
                onSelect: () => handleUpdateRig(makeRoomPreset(preset, rig), true),
              })),
            ],
          },
          {
            kind: 'submenu',
            id: 'proc-stage5',
            label: `Stage 5 — Optional Plugin Slot (${
              pipelineStages.pluginSlot.enabled && pipelineStages.pluginSlot.pluginId !== 'none'
                ? pipelineStages.pluginSlot.pluginId.replace('_', ' ')
                : 'OFF'
            })`,
            items: [
              {
                kind: 'item',
                id: 'proc-toggle-plugin',
                label: 'Enable Stage 5 Plugin Slot',
                checked:
                  pipelineStages.pluginSlot.enabled &&
                  pipelineStages.pluginSlot.pluginId !== 'none',
                onSelect: () =>
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
                  })),
              },
              {
                kind: 'item',
                id: 'proc-open-plugin-inspector',
                label: 'Open Plugin Slot Inspector',
                onSelect: () => {
                  openRightPanelTab('inspector');
                  setInspectorTab('source');
                },
              },
            ],
          },
          { kind: 'separator' },
          {
            kind: 'item',
            id: 'proc-render-pipeline',
            label: pipelineBusy
              ? 'Rendering Active Pipeline…'
              : 'Render Active Pipeline (Stages 3–5)',
            disabled: !session || pipelineBusy,
            onSelect: handleRerenderCapture,
          },
          {
            kind: 'item',
            id: 'proc-analyze-layers',
            label: 'Analyze Inspection Layers',
            disabled: !session || layersBusy,
            onSelect: () => {
              openRightPanelTab('diagnostics');
              setDockTab('layers');
              void handleAnalyzeLayers();
            },
          },
          {
            kind: 'item',
            id: 'proc-generate-report',
            label: reportBusy
              ? 'Auditing Before/After Report…'
              : 'Generate Blind Before/After Technical Report…',
            disabled: !session || reportBusy,
            onSelect: () => void handleGenerateTechnicalReport(),
          },
        ],
      },
      {
        id: 'help',
        label: 'Help',
        items: [
          {
            kind: 'item',
            id: 'help-search',
            label: 'Search Help & Jump to Tool…',
            shortcut: 'F1',
            onSelect: () => openHelp('search'),
          },
          {
            kind: 'item',
            id: 'help-glossary',
            label: 'Glossary & Definitions…',
            onSelect: () => openHelp('glossary'),
          },
          { kind: 'separator' },
          {
            kind: 'item',
            id: 'help-check-updates',
            label: updateReport?.has_staged_download
              ? 'Check for Updates & Staged Releases… (Staged Ready)'
              : updateReport?.has_updates
                ? 'Check for Updates & Staged Releases… (Update Available)'
                : 'Check for Updates & Staged Releases…',
            onSelect: () => {
              openSettings('updates');
              void handleCheckUpdates(true);
            },
          },
          {
            kind: 'item',
            id: 'help-artcraft-settings',
            label: 'ArtCraft & craft-libs Settings…',
            shortcut: 'Ctrl+,',
            onSelect: () => openSettings('appearance'),
          },
          { kind: 'separator' },
          {
            kind: 'submenu',
            id: 'help-docs',
            label: 'READMEs & Technical Notes',
            items: [
              {
                kind: 'item',
                id: 'help-doc-readme',
                label: 'README — Project Overview',
                onSelect: () => openHelp('docs', { docId: 'readme' }),
              },
              {
                kind: 'item',
                id: 'help-doc-spec',
                label: 'SPECTRACRAFT_SPEC.md — Architecture & Invariants',
                onSelect: () => openHelp('docs', { docId: 'spec' }),
              },
              {
                kind: 'item',
                id: 'help-doc-craft-libs',
                label: 'CRAFT_LIBS_MAINTENANCE.md — Pinned Dependencies',
                onSelect: () => openHelp('docs', { docId: 'craft-libs' }),
              },
              {
                kind: 'item',
                id: 'help-doc-design',
                label: 'DESIGN-CONTRACT.md — Visual & Layout Contract',
                onSelect: () => openHelp('docs', { docId: 'design-contract' }),
              },
              {
                kind: 'item',
                id: 'help-doc-platform',
                label: 'PLATFORM.md — Desktop Platform Notes',
                onSelect: () => openHelp('docs', { docId: 'platform' }),
              },
              {
                kind: 'item',
                id: 'help-doc-plugin',
                label: 'PLUGIN_ARCHITECTURE.md — Stage 5 Plugin Contract',
                onSelect: () => openHelp('docs', { docId: 'plugin' }),
              },
            ],
          },
          { kind: 'separator' },
          {
            kind: 'item',
            id: 'help-lineage',
            label: 'Resynthesis Lineage & Scope…',
            onSelect: () => setLineageModalOpen(true),
          },
          {
            kind: 'item',
            id: 'help-about',
            label: 'About SpectraCraft…',
            onSelect: () => openHelp('about'),
          },
        ],
      },
    ],
    [
      session,
      reportBusy,
      technicalReport,
      rigHistory.past.length,
      rigHistory.future.length,
      activeTool,
      regionHistory.regions.length,
      regionHistory.past.length,
      regionHistory.future.length,
      selection,
      isPlaying,
      loadingAudio,
      takeMode,
      capturedBuffer,
      levelMatchAb,
      workspacePrefs,
      displayMaxHz,
      spectrogramDbFloor,
      previewBitDepth,
      showEvidenceOverlay,
      durationS,
      dockTab,
      cuesBusy,
      sourceRegenBusy,
      pipelineStages,
      rig,
      pipelineBusy,
      layersBusy,
      updateReport,
      handleOpenReference,
      handleGenerateTechnicalReport,
      handleCheckUpdates,
      handleCloseFile,
      handleUndo,
      handleRedo,
      persistWorkspacePatch,
      openRightPanelTab,
      openSettings,
      handleQuickAddDryRegion,
      handleUpdateRig,
      handleTogglePlay,
      handleSeek,
      handleSwitchTake,
      handleRenderEditedCues,
      handleRegenerateDryWithSource,
      openDryReviewDock,
      handleRerenderCapture,
      handleAnalyzeLayers,
      openHelp,
    ],
  );

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

      {/* Single Clean ArtCraft Top Menu Bar — no extra toolbar or update banner below */}
      <MenuBar
        menus={menus}
        openMenuId={openMenu}
        onOpenMenuChange={(id) => setOpenMenu(id as MenuId)}
        rightMeta={rightMeta}
        playheadReadout={playheadReadout}
      />

      {/* Main ArtCraft / Adobe-Style Desktop Body: Left Tool Palette | Audio Canvas | Right Tabbed Panel */}
      <div className="flex-1 min-h-0 flex relative">
        {/* Left Vertical Tool Palette (ArtCraft / Adobe style, toggled via Workspace menu) */}
        {workspacePrefs.show_tool_rail && (
          <nav
            aria-label="Left Tool Palette"
            data-testid="left-tool-palette"
            className="w-11 bg-[#25272d] border-r border-[#383b44] flex flex-col items-center py-2 gap-1 shrink-0"
          >
            {/* Direct Canvas Manipulation Tools */}
            <ToolButton
              icon={<MousePointer2 className="w-4 h-4" />}
              label="Pointer / Scrub Tool (V)"
              shortcut="V"
              testId="tool-btn-scrub"
              active={activeTool === 'scrub'}
              onClick={() => setActiveTool('scrub')}
            />
            <ToolButton
              icon={<BoxSelect className="w-4 h-4" />}
              label="Rectangular Marquee Tool — Stage 2 Dry (M)"
              shortcut="M"
              testId="tool-btn-marquee"
              active={activeTool === 'marquee'}
              onClick={() => setActiveTool('marquee')}
            />
            <ToolButton
              icon={<Crosshair className="w-4 h-4" />}
              label="In / Out Range Select Tool — Stage 2 Dry (R)"
              shortcut="R"
              testId="tool-btn-range"
              active={activeTool === 'range'}
              onClick={() => setActiveTool('range')}
            />
            <ToolButton
              icon={<Search className="w-4 h-4" />}
              label="Zoom Tool — Click In / Shift+Click Out (Z)"
              shortcut="Z"
              testId="tool-btn-zoom"
              active={activeTool === 'zoom'}
              onClick={() => setActiveTool('zoom')}
            />

            <div className="w-6 h-px bg-[#3a3e48] my-1.5" />

            {/* Canvas View Mode Toggles */}
            <ToolButton
              icon={<AudioWaveform className="w-4 h-4" />}
              label="Toggle Waveform View"
              testId="tool-btn-waveform"
              active={workspacePrefs.show_waveform}
              onClick={() =>
                persistWorkspacePatch({ show_waveform: !workspacePrefs.show_waveform })
              }
            />
            <ToolButton
              icon={<BarChart3 className="w-4 h-4" />}
              label="Toggle Spectrogram View"
              testId="tool-btn-spectrogram"
              active={workspacePrefs.show_spectrogram}
              onClick={() =>
                persistWorkspacePatch({ show_spectrogram: !workspacePrefs.show_spectrogram })
              }
            />
            <ToolButton
              icon={<Eye className="w-4 h-4" />}
              label="Toggle Cue Evidence Overlay (Visual Only)"
              testId="tool-btn-evidence"
              active={showEvidenceOverlay}
              onClick={() => setShowEvidenceOverlay((v) => !v)}
            />

            <div className="w-6 h-px bg-[#3a3e48] my-1.5" />

            {/* Right Tabbed Panel Quick-Switch */}
            <ToolButton
              icon={<SlidersHorizontal className="w-4 h-4" />}
              label="Inspector Tab (Stages 1, 3–5)"
              testId="tool-btn-panel-inspector"
              active={
                workspacePrefs.show_right_panel && workspacePrefs.right_panel_tab === 'inspector'
              }
              onClick={() => openRightPanelTab('inspector')}
            />
            <ToolButton
              icon={<Activity className="w-4 h-4" />}
              label="Diagnostics & Review Tab"
              testId="tool-btn-panel-diagnostics"
              active={
                workspacePrefs.show_right_panel && workspacePrefs.right_panel_tab === 'diagnostics'
              }
              onClick={() => openRightPanelTab('diagnostics')}
            />
            <ToolButton
              icon={<FileText className="w-4 h-4" />}
              label="Before/After Technical Report Tab"
              testId="tool-btn-panel-report"
              active={
                workspacePrefs.show_right_panel && workspacePrefs.right_panel_tab === 'report'
              }
              onClick={() => openRightPanelTab('report')}
            />
            <ToolButton
              icon={<Box className="w-4 h-4" />}
              label="Room Views Tab (Plans & 3D)"
              testId="tool-btn-panel-room"
              active={workspacePrefs.show_right_panel && workspacePrefs.right_panel_tab === 'room'}
              onClick={() => openRightPanelTab('room')}
            />
          </nav>
        )}

        {/* Main Center Canvas + Right Tabbed Panel Area */}
        <main className="flex-1 min-w-0 min-h-0 flex flex-col bg-[#1a1a1a] relative">
          {(() => {
            const inspectorNode = (
              <StudioInspector
                rig={rig}
                catalogs={catalogs}
                activeTab={inspectorTab}
                onTabChange={setInspectorTab}
                onUpdateRig={handleUpdateRig}
                livePreview={livePreview}
                onToggleLivePreview={setLivePreview}
                acousticAnalysis={
                  rustAcousticAnalysis ??
                  pipelineResult?.acoustic_analysis ??
                  null
                }
                analysisFromRust={Boolean(
                  rustAcousticAnalysis ?? pipelineResult?.acoustic_analysis,
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
                dryPassedCount={dryPassedCount}
                onOpenDryReview={openDryReviewDock}
                onGenerateReport={() => void handleGenerateTechnicalReport()}
                onOpenReport={() => openRightPanelTab('report')}
                hasReport={Boolean(technicalReport)}
                reportBusy={reportBusy}
              />
            );

            const spatialNode = (
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
                onChangeSpatialMode={(m) => persistWorkspacePatch({ spatial_mode: m })}
              />
            );

            const buildDiagnosticsPanel = (embeddedInSidePanel: boolean) => (
              <StudioBottomDock
                collapsed={false}
                embeddedInSidePanel={embeddedInSidePanel}
                bodyHeightPx={Math.max(80, layout.bottomDockHeight - 32)}
                onToggleCollapse={() =>
                  persistWorkspacePatch({ show_diagnostics_dock: false })
                }
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
                  setLayerChannels((prev) =>
                    prev.map((c) => (c.id === id ? { ...c, ...patch } : c)),
                  )
                }
                onToggleViewLayer={(id) =>
                  setViewedLayerId((curr) => (curr === id ? null : id))
                }
                onAnalyzeLayers={() => void handleAnalyzeLayers()}
                layersBusy={layersBusy}
                hasLayerStems={Boolean(layerSession)}
                layerMixPlaying={layerMixPlaying}
                onToggleLayerMixPlay={handleToggleLayerMixPlay}
                notes={session?.notes ?? []}
                cuePatches={cuePatches}
                onUpdateCuePatch={(idx, patch) =>
                  setCuePatches((prev) => ({ ...prev, [idx]: patch }))
                }
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
                    : sourceRegenResult?.drySha256 ??
                      session?.verification.dry_candidate_sha256 ??
                      null
                }
                capturedSha256={pipelineResult?.wav_sha256 ?? null}
                onOpenLineageModal={() => setLineageModalOpen(true)}
                onApplyCharacterGuess={handleApplyCharacterGuess}
                dryRegions={regionHistory.regions}
                selectedRegionId={regionHistory.selectedId}
                canUndoRegion={regionHistory.past.length > 0}
                canRedoRegion={regionHistory.future.length > 0}
                onSelectDryRegion={(id) =>
                  setRegionHistory((prev) => selectDryRegion(prev, id))
                }
                onQuickAddDryRegion={handleQuickAddDryRegion}
                onUpdateDryRegion={(id, patch) =>
                  setRegionHistory((prev) => updateDryRegion(prev, id, patch, durationS, true))
                }
                onDeleteDryRegion={(id) => {
                  stopRegionLoop();
                  setRegionHistory((prev) => deleteDryRegion(prev, id));
                }}
                onUndoDryRegion={() =>
                  setRegionHistory((prev) => undoDryRegionHistory(prev))
                }
                onRedoDryRegion={() =>
                  setRegionHistory((prev) => redoDryRegionHistory(prev))
                }
                onPreviewDryRegion={(reg) => void runDryRegionEdit(reg, false)}
                onApplyDryRegion={(reg) => void runDryRegionEdit(reg, true)}
                regionEditBusy={regionEditBusy}
                regionEditResult={regionEditResult}
                regionPreviewPlaying={regionPreviewPlaying}
                regionPreviewAb={regionPreviewAb}
                onToggleRegionPreviewPlay={handleToggleRegionPreviewPlay}
                onChangeRegionPreviewAb={handleChangeRegionPreviewAb}
                onGenerateReport={() => void handleGenerateTechnicalReport()}
                onOpenReport={() => openRightPanelTab('report')}
                hasReport={Boolean(technicalReport)}
                reportBusy={reportBusy}
              />
            );

            const canvasVisible = workspacePrefs.show_waveform || workspacePrefs.show_spectrogram;

            const timelineNode = (
              <div className="flex-1 min-w-[260px] flex flex-col min-h-0">
                {/* Optional inline Transport Strip (enabled via Workspace -> Inline Transport Bar) */}
                {workspacePrefs.show_transport && (
                  <div
                    data-testid="workspace-transport-bar"
                    className="h-9 bg-[#23252b] border-b border-[#383b44] px-2.5 flex items-center justify-between gap-2 shrink-0 overflow-x-auto"
                  >
                    <div className="flex items-center gap-1.5">
                      <button
                        type="button"
                        disabled={!session || loadingAudio}
                        onClick={() => handleSeek(0)}
                        className="h-6 w-6 bg-[#2e3138] hover:bg-[#3a3e48] disabled:opacity-35 border border-[#444854] flex items-center justify-center"
                        title="Rewind to start (Home)"
                      >
                        <SkipBack className="w-3.5 h-3.5" />
                      </button>
                      <button
                        type="button"
                        disabled={!session || loadingAudio}
                        onClick={handleTogglePlay}
                        className={`h-6 px-2.5 border flex items-center gap-1.5 text-[11px] font-medium disabled:opacity-35 ${
                          isPlaying
                            ? 'bg-[#2f6ec4] border-[#2f6ec4] text-white'
                            : 'bg-[#2e3138] hover:bg-[#3a3e48] border-[#444854] text-[#e8ecf4]'
                        }`}
                        title="Play / Pause (Space)"
                      >
                        {isPlaying ? <Pause className="w-3 h-3" /> : <Play className="w-3 h-3" />}
                        <span>{isPlaying ? 'Pause' : 'Play'}</span>
                      </button>
                      <span className="font-mono text-[11px] tabular-nums text-[#cfd6e2] px-1.5">
                        {playheadS.toFixed(3)}s / {durationS.toFixed(3)}s
                      </span>
                      <div className="flex items-center border border-[#444854]">
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
                            className={`h-6 px-2 text-[11px] border-r last:border-r-0 border-[#444854] disabled:opacity-35 ${
                              takeMode === mode
                                ? 'bg-[#2f6ec4] text-white font-medium'
                                : 'bg-[#2c2f36] text-[#c8cdd6] hover:bg-[#373b44]'
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
                        className={`h-6 px-2 border font-mono text-[10px] disabled:opacity-35 ${
                          levelMatchAb
                            ? 'bg-[#20332d] border-[#3a7c63] text-[#6cd4b4]'
                            : 'bg-[#2c2f36] border-[#444854] text-[#9ea4b0]'
                        }`}
                      >
                        Match dB
                      </button>
                    </div>
                    <div className="flex items-center border border-[#444854] shrink-0">
                      <button
                        type="button"
                        disabled={!session}
                        onClick={() => zoomBy(0.5)}
                        className="h-6 w-6 bg-[#2c2f36] hover:bg-[#373b44] disabled:opacity-35 flex items-center justify-center border-r border-[#444854]"
                        title="Zoom in"
                      >
                        <ZoomIn className="w-3.5 h-3.5" />
                      </button>
                      <button
                        type="button"
                        disabled={!session}
                        onClick={() => zoomBy(2.0)}
                        className="h-6 w-6 bg-[#2c2f36] hover:bg-[#373b44] disabled:opacity-35 flex items-center justify-center border-r border-[#444854]"
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
                        className="h-6 px-2 bg-[#2c2f36] hover:bg-[#373b44] disabled:opacity-35 text-[11px]"
                        title="Fit full recording"
                      >
                        Fit
                      </button>
                    </div>
                  </div>
                )}

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
                ) : !canvasVisible ? (
                  <div
                    data-testid="canvas-hidden-placeholder"
                    className="flex-1 flex flex-col items-center justify-center gap-3 p-6 text-center text-[#9ba3b0]"
                  >
                    <p className="text-[13px] text-[#cfd6e2]">
                      Waveform and Spectrogram views are hidden in Workspace.
                    </p>
                    <button
                      type="button"
                      onClick={() =>
                        persistWorkspacePatch({ show_waveform: true, show_spectrogram: true })
                      }
                      className="h-7 px-3 bg-[#2f6ec4] hover:bg-[#275ea8] text-white text-[12px]"
                    >
                      Show Waveform & Spectrogram
                    </button>
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

            const rightPanelWidth = Math.max(320, Math.min(560, layout.spatialWidth || 380));

            return (
              <div className="flex-1 min-h-0 flex">
                {/* Center Main Canvas Column (Audio Viewport + Optional Bottom Diagnostics Dock) */}
                <div className="flex-1 min-w-0 min-h-0 flex flex-col">
                  {timelineNode}

                  {workspacePrefs.show_diagnostics_dock && (
                    <>
                      <SplitterHandle
                        orientation="horizontal"
                        label="Resize bottom dock height"
                        onDragDelta={(_dx, dy) =>
                          setLayout((prev) =>
                            resizeBottomDockHeight(prev, prev.bottomDockHeight - dy),
                          )
                        }
                      />
                      <div data-testid="workspace-diagnostics-dock" className="shrink-0">
                        {buildDiagnosticsPanel(false)}
                      </div>
                    </>
                  )}
                </div>

                {/* Right-Side Tabbed Panel Area (Photoshop / Illustrator / ArtCraft style) */}
                {workspacePrefs.show_right_panel && (
                  <>
                    <SplitterHandle
                      orientation="vertical"
                      label="Resize right tabbed panel"
                      onDragDelta={(dx) =>
                        setLayout((prev) => resizeSpatialWidth(prev, prev.spatialWidth - dx))
                      }
                    />
                    <div
                      style={{ width: `${rightPanelWidth}px` }}
                      className="shrink-0 min-h-0 flex flex-col"
                    >
                      <RightTabbedPanel
                        activeTab={workspacePrefs.right_panel_tab}
                        onTabChange={(tab) => openRightPanelTab(tab)}
                        onHidePanel={() => persistWorkspacePatch({ show_right_panel: false })}
                        inspectorContent={inspectorNode}
                        diagnosticsContent={buildDiagnosticsPanel(true)}
                        roomContent={spatialNode}
                        hasSession={Boolean(session)}
                        report={technicalReport}
                        reportBusy={reportBusy}
                        reportError={reportError}
                        onGenerateReport={() => void handleGenerateTechnicalReport()}
                        onExpandReportModal={() => setReportModalOpen(true)}
                        onExportReport={handleExportTechnicalReport}
                        reportExportBusy={reportExportBusy}
                        reportLastExport={reportLastExport}
                        defaultExportFormat={
                          settingsSnapshot?.settings.report.default_export_format ?? 'pdf'
                        }
                        darkMode={settingsSnapshot?.settings.report.dark_mode_viewer ?? true}
                      />
                    </div>
                  </>
                )}
              </div>
            );
          })()}
        </main>
      </div>

      {/* Quiet Desktop Status Bar (24px) */}
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

        <div className="flex items-center gap-2 shrink-0 text-[#8a8a8a]">
          {session && (
            <>
              <button
                type="button"
                disabled={reportBusy || pipelineBusy}
                data-testid="status-generate-report-btn"
                onClick={() => void handleGenerateTechnicalReport()}
                className="h-4 px-1.5 bg-[#1f2c3d] hover:bg-[#2a3d56] border border-[#365378] text-[#cfe4ff] text-[10px] font-sans disabled:opacity-40"
                title="Run blind Before/After technical audit and open dark-mode Report viewer"
              >
                {reportBusy ? 'Auditing…' : 'Generate Report'}
              </button>
              {technicalReport && (
                <button
                  type="button"
                  data-testid="status-open-report-btn"
                  onClick={() => openRightPanelTab('report')}
                  className="h-4 px-1.5 bg-[#122a24] hover:bg-[#193b33] border border-[#2d705e] text-[#7ae9c6] text-[10px] font-sans"
                  title="Open docked dark-mode Report viewer"
                >
                  Open Report
                </button>
              )}
              <span className="text-[#565656]">|</span>
            </>
          )}
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
        activeCuesRunId={activeCuesRunId}
        pipelineResult={pipelineResult}
      />

      <ResynthesisLineageModal
        open={lineageModalOpen}
        onClose={() => setLineageModalOpen(false)}
      />

      <TechnicalReportModal
        open={reportModalOpen}
        onClose={() => setReportModalOpen(false)}
        report={technicalReport}
        busy={reportBusy}
        error={reportError}
        onRegenerate={() => void handleGenerateTechnicalReport()}
        onExport={handleExportTechnicalReport}
        exportBusyFormat={reportExportBusy}
        lastExport={reportLastExport}
        darkMode={settingsSnapshot?.settings.report.dark_mode_viewer ?? true}
        defaultExportFormat={settingsSnapshot?.settings.report.default_export_format ?? 'pdf'}
      />

      <SettingsModal
        open={settingsModalOpen}
        focusSection={settingsFocusSection}
        onClose={() => {
          setSettingsModalOpen(false);
          setSettingsFocusSection(null);
        }}
        snapshot={settingsSnapshot}
        saving={settingsSaving}
        error={settingsError}
        onSave={handleSaveSettings}
        onResetDefaults={handleResetSettings}
        updateReport={updateReport}
        updateCheckBusy={updateCheckBusy}
        updateActionBusy={updateActionBusy}
        updateActionResult={updateActionResult}
        onCheckUpdates={handleCheckUpdates}
        onUpdateAction={handleUpdateAction}
      />

      <HelpModal
        open={helpModalOpen}
        initialTab={helpInitialTab}
        initialQuery={helpInitialQuery}
        initialDocId={helpInitialDocId}
        onClose={() => setHelpModalOpen(false)}
        onNavigateAction={handleHelpNavigate}
      />
    </div>
  );
}

function ToolButton({
  icon,
  label,
  shortcut,
  testId,
  active,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  shortcut?: string;
  testId?: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      aria-label={label}
      aria-pressed={active}
      data-testid={testId}
      data-active={active ? 'true' : 'false'}
      className={`relative w-8 h-8 flex items-center justify-center border transition-colors ${
        active
          ? 'bg-[#2f6ec4] border-[#5da2f8] text-white'
          : 'bg-transparent border-transparent text-[#a8b0be] hover:bg-[#32353f] hover:border-[#424652] hover:text-[#f2f5fb]'
      }`}
    >
      {active && <span className="absolute left-0 top-1 bottom-1 w-[2px] bg-[#99c8ff]" />}
      {icon}
      {shortcut && (
        <span
          className={`absolute bottom-[1px] right-[2px] text-[8px] font-mono leading-none ${
            active ? 'text-[#dbeafe]' : 'text-[#788090]'
          }`}
        >
          {shortcut}
        </span>
      )}
    </button>
  );
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

