import React, { useMemo, useRef, useState } from 'react';
import {
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  Link2,
  Plus,
  RotateCcw,
  Trash2,
  Unlink2,
} from 'lucide-react';
import {
  ALL_PATTERNS,
  ALL_UNITS,
  BANDS_HZ,
  DEFAULT_PIPELINE_STAGES,
  DEFAULT_VOICING_CONTROLS,
  FEET_TO_METERS,
  anglesPointingAtSource,
  applyStereoPreset,
  clampRig,
  computeAcousticAnalysisLocal,
  displayToMeters,
  getMicWorldPosM,
  getSourcePosM,
  gridDirectionAngles,
  makeDefaultMicSlot,
  makeRoomPreset,
  metersToDisplay,
  polarGain,
  resetSourceToDefault,
  setLinkedDimensionM,
  setMicCatalogIndex,
  setMicWorldPosM,
  setSourcePosM,
  snapAnglesToCardinalWithinThreshold,
  speedOfSoundMps,
  unitDecimals,
  type CharacterTarget,
  type GridDirectionKey,
  type MicSlotState,
  type PipelineRenderResponse,
  type PipelineStagesState,
  type PluginSlotId,
  type PolarPattern,
  type RoomPresetKey,
  type RustAcousticAnalysis,
  type RustCaptureResult,
  type RustSourceRegenResult,
  type RustStudioCatalogs,
  type SignalPathStyle,
  type SourceCharacterVariant,
  type StraightenMode,
  type StudioRigState,
  type VoicingControlsState,
} from '../lib/studioRig.ts';

export type InspectorTab = 'source' | 'mic' | 'materials' | 'room';

function formatHzRange(range: [number, number] | null | undefined): string {
  if (!range) return 'Unspecified';
  const fmt = (hz: number) =>
    hz >= 1000 ? `${(hz / 1000).toFixed(hz % 1000 === 0 ? 0 : 1)} kHz` : `${Math.round(hz)} Hz`;
  return `${fmt(range[0])}–${fmt(range[1])}`;
}

interface StudioInspectorProps {
  rig: StudioRigState;
  catalogs: RustStudioCatalogs | null;
  activeTab: InspectorTab;
  onTabChange: (tab: InspectorTab) => void;
  onUpdateRig: (next: StudioRigState, commit?: boolean) => void;
  livePreview: boolean;
  onToggleLivePreview: (v: boolean) => void;
  onRerenderCapture: () => void;
  captureBusy: boolean;
  captureResult: RustCaptureResult | null;
  acousticAnalysis?: RustAcousticAnalysis | null;
  analysisFromRust?: boolean;
  hasAudio: boolean;
  onRegenerateDryWithSource?: () => void;
  sourceRegenBusy?: boolean;
  sourceRegenResult?: RustSourceRegenResult | null;
  // Stage 1 DDSP Voicing + Extensible Catalog Reload
  voicing?: VoicingControlsState;
  onChangeVoicing?: (next: VoicingControlsState) => void;
  onReloadCatalogs?: () => void;
  reloadingCatalogs?: boolean;
  // Stages 3, 4, 5 Independently Bypassable Post-Dry Pipeline (all OFF by default)
  pipelineStages?: PipelineStagesState;
  onChangePipelineStages?: (next: PipelineStagesState) => void;
  onRenderPipeline?: () => void;
  pipelineBusy?: boolean;
  pipelineResult?: PipelineRenderResponse | null;
  dryReviewApproved: boolean;
  dryPassedCount: number;
  onOpenDryReview: () => void;
  /** ArtCraft workspace chrome callbacks (purely visual — never affect rig or audio). */
  isFloating?: boolean;
  canMoveLeft?: boolean;
  canMoveRight?: boolean;
  onMovePane?: (dir: -1 | 1) => void;
  onToggleFloat?: () => void;
}

const GRID_DIRS: GridDirectionKey[] = ['+X', '-X', '+Y', '-Y', '+Z', '-Z'];

export function StudioInspector({
  rig,
  catalogs,
  activeTab,
  onTabChange,
  onUpdateRig,
  livePreview,
  onToggleLivePreview,
  onRerenderCapture,
  captureBusy,
  captureResult,
  acousticAnalysis = null,
  analysisFromRust = false,
  hasAudio,
  onRegenerateDryWithSource,
  sourceRegenBusy = false,
  sourceRegenResult = null,
  voicing = DEFAULT_VOICING_CONTROLS,
  onChangeVoicing,
  onReloadCatalogs,
  reloadingCatalogs = false,
  pipelineStages = DEFAULT_PIPELINE_STAGES,
  onChangePipelineStages,
  onRenderPipeline,
  pipelineBusy = false,
  pipelineResult = null,
  dryReviewApproved,
  dryPassedCount,
  onOpenDryReview,
  isFloating = false,
  canMoveLeft = false,
  canMoveRight = false,
  onMovePane,
  onToggleFloat,
}: StudioInspectorProps) {
  const [sourceCat, setSourceCat] = useState<'All' | 'Instrument' | 'Human' | 'Unknown'>('Instrument');
  const [sourceFamily, setSourceFamily] = useState<string>('All');
  const [sourceQuery, setSourceQuery] = useState('');

  const [micBrand, setMicBrand] = useState<string>('All');
  const [micQuery, setMicQuery] = useState('');
  const [snapPulseAxis, setSnapPulseAxis] = useState<GridDirectionKey | null>(null);

  const [selectedFace, setSelectedFace] = useState<number>(0);
  const [materialQuery, setMaterialQuery] = useState('');

  const sources = catalogs?.sources ?? [];
  const mics = catalogs?.mics ?? [];
  const surfaces = catalogs?.surfaces ?? [];
  const faceLabels = catalogs?.face_labels ?? [
    '-X Left wall',
    '+X Right wall',
    '-Y Front wall',
    '+Y Back wall',
    'Floor / Ground',
    'Ceiling',
  ];

  const activeSource = sources[rig.sourceIndex] ?? sources[0] ?? null;
  const analysis = useMemo(
    () => acousticAnalysis ?? computeAcousticAnalysisLocal(rig, sources, mics, surfaces),
    [acousticAnalysis, rig, sources, mics, surfaces],
  );
  const activeFaceContribution = analysis.face_contributions[selectedFace] ?? null;
  const activeSlot: MicSlotState = rig.mics[rig.selectedMic] ?? rig.mics[0] ?? makeDefaultMicSlot(0);
  const activeMicEntry = mics[activeSlot.catalogIndex] ?? mics[0] ?? null;
  const activePattern: PolarPattern =
    activeSlot.patternOverride ?? activeMicEntry?.default_pattern ?? 'cardioid';

  const sourceFamilies = useMemo(() => {
    const set = new Set<string>();
    for (const s of sources) {
      if (sourceCat === 'All' || s.category === sourceCat) {
        set.add(s.family);
      }
    }
    return Array.from(set);
  }, [sources, sourceCat]);

  const filteredSources = useMemo(() => {
    const q = sourceQuery.trim().toLowerCase();
    return sources.filter((s) => {
      if (sourceCat !== 'All' && s.category !== sourceCat) return false;
      if (sourceFamily !== 'All' && s.family !== sourceFamily) return false;
      if (q && !`${s.name} ${s.family} ${s.category}`.toLowerCase().includes(q)) return false;
      return true;
    });
  }, [sources, sourceCat, sourceFamily, sourceQuery]);

  const micBrands = useMemo(() => {
    const counts = new Map<string, number>();
    for (const m of mics) {
      counts.set(m.brand, (counts.get(m.brand) ?? 0) + 1);
    }
    return Array.from(counts.entries())
      .sort((a, b) => a[0].localeCompare(b[0]))
      .map(([brand, count]) => ({ brand, count }));
  }, [mics]);

  const filteredMics = useMemo(() => {
    const q = micQuery.trim().toLowerCase();
    return mics.filter((m) => {
      if (micBrand !== 'All' && m.brand !== micBrand) return false;
      if (
        q &&
        !`${m.brand} ${m.model} ${m.transducer} ${m.default_pattern_label} ${m.published_type ?? ''} ${m.published_patterns ?? ''}`
          .toLowerCase()
          .includes(q)
      ) {
        return false;
      }
      return true;
    });
  }, [mics, micBrand, micQuery]);

  const filteredSurfaces = useMemo(() => {
    const q = materialQuery.trim().toLowerCase();
    if (!q) return surfaces;
    return surfaces.filter((s) =>
      `${s.name} ${s.category} ${s.material_type} ${s.assembly} ${s.standard} ${s.notes}`
        .toLowerCase()
        .includes(q),
    );
  }, [surfaces, materialQuery]);

  const effectiveFace = rig.preset === 'open' ? 4 : selectedFace;
  const activeSurface = surfaces[rig.surfaceFaces[effectiveFace]] ?? surfaces[0] ?? null;

  const srcPos = getSourcePosM(rig);
  const micPos = getMicWorldPosM(rig, rig.selectedMic);
  const distM = Math.hypot(srcPos.x - micPos.x, srcPos.y - micPos.y, srcPos.z - micPos.z);
  const dec = unitDecimals(rig.unit);
  const unitSuffix = rig.unit;

  const updateActiveMic = (updater: (m: MicSlotState) => MicSlotState, commit = true) => {
    const nextMics = rig.mics.map((m, i) => (i === rig.selectedMic ? updater(m) : m));
    onUpdateRig(clampRig({ ...rig, mics: nextMics }), commit);
  };

  const nudgeSource = (axis: 'x' | 'y' | 'z', deltaUnits: number) => {
    const deltaM = displayToMeters(deltaUnits, rig.unit);
    onUpdateRig(setSourcePosM(rig, { ...srcPos, [axis]: srcPos[axis] + deltaM }), true);
  };

  const nudgeMic = (axis: 'x' | 'y' | 'z', deltaUnits: number) => {
    const deltaM = displayToMeters(deltaUnits, rig.unit);
    onUpdateRig(
      setMicWorldPosM(rig, rig.selectedMic, { ...micPos, [axis]: micPos[axis] + deltaM }),
      true,
    );
  };

  const tabs: { id: InspectorTab; label: string }[] = [
    { id: 'source', label: 'Source' },
    { id: 'mic', label: 'Microphone' },
    { id: 'materials', label: 'Materials' },
    { id: 'room', label: 'Room' },
  ];

  return (
    <aside className="w-full h-full bg-[#262626] flex flex-col min-h-0">
      {/* Clean 5-Stage Pipeline Header Banner + Panel Dock/Reorder Controls */}
      <div
        className={`h-7 px-2 border-b border-[#3c3c3c] flex items-center justify-between gap-1 text-[11px] shrink-0 ${
          pipelineStages.micEnabled ||
          pipelineStages.roomEnabled ||
          (pipelineStages.pluginSlot.enabled && pipelineStages.pluginSlot.pluginId !== 'none')
            ? 'bg-[#1f2c26] text-[#6cd4b4]'
            : 'bg-[#222831] text-[#8dbef7]'
        }`}
      >
        <span className="flex items-center gap-1.5 font-medium truncate">
          <span className="truncate">
            {pipelineStages.micEnabled ||
            pipelineStages.roomEnabled ||
            (pipelineStages.pluginSlot.enabled && pipelineStages.pluginSlot.pluginId !== 'none')
              ? `Post-Dry Active: ${[
                  pipelineStages.micEnabled ? 'Mic' : null,
                  pipelineStages.roomEnabled ? 'Room' : null,
                  pipelineStages.pluginSlot.enabled && pipelineStages.pluginSlot.pluginId !== 'none'
                    ? 'Plugin'
                    : null,
                ]
                  .filter(Boolean)
                  .join(' + ')}`
              : 'Dry First · Post-Dry Stages OFF'}
          </span>
        </span>
        <div className="flex items-center gap-1 shrink-0">
          <button
            type="button"
            onClick={onOpenDryReview}
            className="text-[10px] underline hover:text-white mr-1"
            title="Inspect dry fidelity checks"
          >
            Checks ({dryPassedCount}/5)
          </button>
          {!isFloating && onMovePane && (
            <>
              <button
                type="button"
                disabled={!canMoveLeft}
                onClick={() => onMovePane(-1)}
                title="Move Inspector panel left (visual only)"
                aria-label="Move Inspector left"
                className="p-0.5 text-[#a6a6a6] hover:text-white disabled:opacity-25"
              >
                <ChevronLeft className="w-3 h-3" />
              </button>
              <button
                type="button"
                disabled={!canMoveRight}
                onClick={() => onMovePane(1)}
                title="Move Inspector panel right (visual only)"
                aria-label="Move Inspector right"
                className="p-0.5 text-[#a6a6a6] hover:text-white disabled:opacity-25"
              >
                <ChevronRight className="w-3 h-3" />
              </button>
            </>
          )}
          {!isFloating && onToggleFloat && (
            <button
              type="button"
              onClick={onToggleFloat}
              title="Undock Inspector into floating window (visual only)"
              aria-label="Undock Inspector"
              className="p-0.5 text-[#a6a6a6] hover:text-white"
            >
              <ExternalLink className="w-3 h-3" />
            </button>
          )}
        </div>
      </div>

      {/* Preview 19 Left-Docked Inspector Tab Strip: Source → Microphone → Materials → Room */}
      <div className="h-8 bg-[#2c2c2c] border-b border-[#3c3c3c] grid grid-cols-4 shrink-0">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            onClick={() => onTabChange(tab.id)}
            className={`text-[11px] font-medium border-r last:border-r-0 border-[#383838] transition-colors truncate px-1 ${
              activeTab === tab.id
                ? 'bg-[#262626] text-white border-b-2 border-b-[#378ef0]'
                : 'text-[#a6a6a6] hover:text-[#e0e0e0] hover:bg-[#323232]'
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Scrollable Active Tab Content */}
      <div className="flex-1 min-h-0 overflow-y-auto p-2.5 space-y-2.5 text-[11px]">
        {/* ================= 1. SOURCE TAB ================= */}
        {activeTab === 'source' && (
          <>
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <span className="text-[10px] uppercase tracking-wider text-[#909090]">
                  Source Catalog ({sources.length} Rust entries)
                </span>
                <span className="text-[10px] text-[#7f7f7f]">1 source at a time</span>
              </div>
              {/* Category row */}
              <div className="grid grid-cols-4 gap-1">
                {(['Instrument', 'Human', 'Unknown', 'All'] as const).map((cat) => (
                  <button
                    key={cat}
                    type="button"
                    onClick={() => {
                      setSourceCat(cat);
                      setSourceFamily('All');
                    }}
                    className={`py-1 border text-[10px] ${
                      sourceCat === cat
                        ? 'bg-[#378ef0] text-white border-[#378ef0]'
                        : 'bg-[#323232] text-[#c2c2c2] border-[#424242] hover:bg-[#3a3a3a]'
                    }`}
                  >
                    {cat}
                  </button>
                ))}
              </div>
              {/* Family cascade */}
              <select
                aria-label="Source Family"
                value={sourceFamily}
                onChange={(e) => setSourceFamily(e.target.value)}
                className="w-full px-1.5 py-1 bg-[#1e1e1e] border border-[#3e3e3e] text-[#dcdcdc] text-[11px]"
              >
                <option value="All">All families ({sourceFamilies.length})</option>
                {sourceFamilies.map((f) => (
                  <option key={f} value={f}>
                    {f}
                  </option>
                ))}
              </select>
              <input
                type="text"
                value={sourceQuery}
                onChange={(e) => setSourceQuery(e.target.value)}
                placeholder="Search 138 sources (e.g. Electric Guitar, Cello, Male)…"
                className="w-full px-2 py-1 bg-[#1e1e1e] border border-[#3e3e3e] text-[#dcdcdc] placeholder-[#707070]"
              />
              <div className="max-h-32 overflow-y-auto border border-[#3a3a3a] bg-[#1e1e1e] divide-y divide-[#2a2a2a]">
                {filteredSources.map((src) => (
                  <button
                    key={src.id}
                    type="button"
                    onClick={() => onUpdateRig({ ...rig, sourceIndex: src.index }, true)}
                    className={`w-full text-left px-2 py-1 flex items-center justify-between gap-2 ${
                      rig.sourceIndex === src.index
                        ? 'bg-[#378ef0] text-white'
                        : 'text-[#cccccc] hover:bg-[#2c2c2c]'
                    }`}
                  >
                    <span className="truncate">{src.name}</span>
                    <span className="text-[9px] opacity-75 shrink-0">{src.family.split('—')[0].trim()}</span>
                  </button>
                ))}
              </div>
            </div>

            {/* Source Placement & Locks */}
            <div className="space-y-1.5 border-t border-[#363636] pt-2">
              <div className="flex items-center justify-between">
                <span className="text-[10px] uppercase tracking-wider text-[#909090]">
                  Source Position ({unitSuffix}) · Floor: {metersToDisplay(srcPos.z, rig.unit).toFixed(dec)}{' '}
                  {unitSuffix}
                </span>
                <button
                  type="button"
                  onClick={() => onUpdateRig(resetSourceToDefault(rig), true)}
                  className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] bg-[#323232] border border-[#454545] text-[#d0d0d0] hover:bg-[#3c3c3c]"
                  title="Reset source to center of footprint and 5 ft above floor"
                >
                  <RotateCcw className="w-2.5 h-2.5" />
                  Center 5 ft
                </button>
              </div>
              <div className="grid grid-cols-3 gap-1.5">
                {(['x', 'y', 'z'] as const).map((axis) => {
                  const locked =
                    axis === 'z' ? rig.sourceLockHeight5ft : rig.sourceLockCenterXY;
                  return (
                    <div key={axis} className="flex flex-col gap-0.5">
                      <div className="flex items-center justify-between text-[10px] text-[#8d8d8d]">
                        <span className="uppercase">{axis}</span>
                        <span className="flex gap-0.5">
                          <button
                            type="button"
                            disabled={locked}
                            onClick={() => nudgeSource(axis, -0.5)}
                            className="px-1 bg-[#2e2e2e] hover:bg-[#3a3a3a] disabled:opacity-35"
                          >
                            −
                          </button>
                          <button
                            type="button"
                            disabled={locked}
                            onClick={() => nudgeSource(axis, 0.5)}
                            className="px-1 bg-[#2e2e2e] hover:bg-[#3a3a3a] disabled:opacity-35"
                          >
                            +
                          </button>
                        </span>
                      </div>
                      <input
                        type="number"
                        step={0.1}
                        disabled={locked}
                        aria-label={`Source ${axis.toUpperCase()}`}
                        value={Number(metersToDisplay(srcPos[axis], rig.unit).toFixed(dec))}
                        onChange={(e) => {
                          const m = displayToMeters(Number(e.target.value) || 0, rig.unit);
                          onUpdateRig(setSourcePosM(rig, { ...srcPos, [axis]: m }), true);
                        }}
                        className="w-full px-1.5 py-1 bg-[#1e1e1e] border border-[#3e3e3e] font-mono tabular-nums text-[#e0e0e0] disabled:opacity-45"
                      />
                    </div>
                  );
                })}
              </div>
              <div className="flex items-center justify-between gap-2 text-[10px] text-[#bcbcbc]">
                <label className="flex items-center gap-1 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={rig.sourceLockCenterXY}
                    onChange={(e) =>
                      onUpdateRig(clampRig({ ...rig, sourceLockCenterXY: e.target.checked }), true)
                    }
                    className="accent-[#378ef0] w-3 h-3"
                  />
                  <span>Lock center (X/Y)</span>
                </label>
                <label className="flex items-center gap-1 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={rig.sourceLockHeight5ft}
                    onChange={(e) =>
                      onUpdateRig(clampRig({ ...rig, sourceLockHeight5ft: e.target.checked }), true)
                    }
                    className="accent-[#378ef0] w-3 h-3"
                  />
                  <span>Lock height (5 ft)</span>
                </label>
              </div>
              <div className="flex items-center justify-between gap-2 text-[10px]">
                <span className="text-[#969696]">Visual Facing (visual only)</span>
                <input
                  type="range"
                  min={-180}
                  max={180}
                  value={rig.sourceFacingDeg}
                  onChange={(e) =>
                    onUpdateRig({ ...rig, sourceFacingDeg: Number(e.target.value) }, false)
                  }
                  className="w-28 studio-slider"
                />
                <span className="font-mono tabular-nums text-[#cccccc] w-8 text-right">
                  {rig.sourceFacingDeg}°
                </span>
              </div>
            </div>

            {/* Stage 1 DDSP Voicing Controls (Dry Stage Only: Character, Path, Variant, Light Straightening) */}
            {activeSource && onRegenerateDryWithSource && (
              <div className="border-t border-[#363636] pt-2 space-y-1.5 text-[10px]">
                <div className="bg-[#1d2126] border border-[#323a44] p-2 space-y-1.5">
                  <div className="flex items-center justify-between gap-1">
                    <span className="uppercase tracking-wider text-[#8ab4e0] font-semibold text-[9.5px]">
                      Stage 1 · Dry Voicing &amp; Light Straightening
                    </span>
                    {onReloadCatalogs && (
                      <button
                        type="button"
                        disabled={reloadingCatalogs}
                        onClick={onReloadCatalogs}
                        className="px-1.5 py-0.5 bg-[#2a3038] hover:bg-[#363f4a] border border-[#44505e] text-[9px] text-[#c0d4e8]"
                        title="Reload extensible source catalog from capture-reference JSON"
                      >
                        {reloadingCatalogs ? 'Reloading…' : `Reload (${sources.length})`}
                      </button>
                    )}
                  </div>

                  <div className="grid grid-cols-2 gap-1.5">
                    <label className="flex flex-col gap-0.5">
                      <span className="text-[9px] text-[#8d9baa]">Character</span>
                      <select
                        aria-label="Dry Character Target"
                        value={voicing.characterTarget}
                        onChange={(e) =>
                          onChangeVoicing?.({
                            ...voicing,
                            characterTarget: e.target.value as CharacterTarget,
                          })
                        }
                        className="px-1.5 py-1 bg-[#16191d] border border-[#39424d] text-[#dce4ec] text-[10px]"
                      >
                        <option value="keep">Keep Extracted</option>
                        <option value="natural">Natural / Acoustic</option>
                        <option value="analog">Analog / Warm</option>
                        <option value="digital">Clean Digital</option>
                      </select>
                    </label>
                    <label className="flex flex-col gap-0.5">
                      <span className="text-[9px] text-[#8d9baa]">Signal Path</span>
                      <select
                        aria-label="Dry Signal Path"
                        value={voicing.signalPath}
                        onChange={(e) =>
                          onChangeVoicing?.({
                            ...voicing,
                            signalPath: e.target.value as SignalPathStyle,
                          })
                        }
                        className="px-1.5 py-1 bg-[#16191d] border border-[#39424d] text-[#dce4ec] text-[10px]"
                      >
                        <option value="default">Default Path</option>
                        <option value="mic_d">Mic&apos;d Acoustic</option>
                        <option value="pickup_direct">Pickup / Direct DI</option>
                        <option value="analog_preamp">Analog Preamp</option>
                        <option value="clean_digital">Clean Digital</option>
                      </select>
                    </label>
                  </div>

                  <div className="grid grid-cols-2 gap-1.5">
                    <label className="flex flex-col gap-0.5">
                      <span className="text-[9px] text-[#8d9baa]">Source Variant</span>
                      <select
                        aria-label="Source Character Variant"
                        value={voicing.sourceVariant}
                        onChange={(e) =>
                          onChangeVoicing?.({
                            ...voicing,
                            sourceVariant: e.target.value as SourceCharacterVariant,
                          })
                        }
                        className="px-1.5 py-1 bg-[#16191d] border border-[#39424d] text-[#dce4ec] text-[10px]"
                      >
                        <option value="standard">Standard</option>
                        <option value="rich">Rich / Full</option>
                        <option value="basic">Basic / Simple</option>
                        <option value="warm">Warm</option>
                        <option value="bright">Bright / Present</option>
                      </select>
                    </label>
                    <label className="flex flex-col gap-0.5">
                      <span className="text-[9px] text-[#8d9baa]">Light Straighten</span>
                      <select
                        aria-label="Light Straightening Mode"
                        value={voicing.straightenMode}
                        onChange={(e) =>
                          onChangeVoicing?.({
                            ...voicing,
                            straightenMode: e.target.value as StraightenMode,
                          })
                        }
                        className="px-1.5 py-1 bg-[#16191d] border border-[#39424d] text-[#dce4ec] text-[10px]"
                      >
                        <option value="off">Off (Exact Extracted)</option>
                        <option value="light">Light (Mild Steady)</option>
                        <option value="moderate">Moderate (Wonky Stems)</option>
                      </select>
                    </label>
                  </div>

                  <div className="flex items-center justify-between gap-2 pt-0.5">
                    <label className="flex items-center gap-1.5 text-[9.5px] text-[#b4c4d4] cursor-pointer">
                      <input
                        type="checkbox"
                        checked={voicing.straightenPreserveBends}
                        onChange={(e) =>
                          onChangeVoicing?.({
                            ...voicing,
                            straightenPreserveBends: e.target.checked,
                          })
                        }
                        className="accent-[#378ef0] w-3 h-3"
                      />
                      <span>Preserve bends &amp; vibrato</span>
                    </label>
                    <div className="flex items-center gap-1.5 font-mono text-[9px] text-[#9eb2c6]">
                      <span>Profile</span>
                      <input
                        type="range"
                        min={0}
                        max={1}
                        step={0.05}
                        value={voicing.profileGuidanceStrength}
                        onChange={(e) =>
                          onChangeVoicing?.({
                            ...voicing,
                            profileGuidanceStrength: Number(e.target.value),
                          })
                        }
                        className="w-16 studio-slider"
                        title="Loose reference profile guidance strength (softly guides voicing without replacing extracted shape)"
                      />
                      <span>{Math.round(voicing.profileGuidanceStrength * 100)}%</span>
                    </div>
                  </div>
                </div>

                <button
                  type="button"
                  disabled={!hasAudio || sourceRegenBusy}
                  onClick={onRegenerateDryWithSource}
                  className="w-full py-1.5 px-2 bg-[#378ef0] hover:bg-[#499af5] disabled:opacity-40 text-white font-medium text-[11px] border border-[#4c9ef7] transition-colors"
                >
                  {sourceRegenBusy
                    ? 'Regenerating Dry Take in Rust…'
                    : `Stage 1 · Regenerate Dry Take (${activeSource.name})`}
                </button>
                {sourceRegenResult && (
                  <div className="bg-[#1b2722] border border-[#2f4d40] p-1.5 font-mono text-[9px] text-[#9ad9be] space-y-0.5">
                    <div className="text-[#d6f5e7] font-sans font-medium text-[10px]">
                      Cached Dry Take: {sourceRegenResult.sourceName}
                    </div>
                    <div>
                      Centroid {Math.round(sourceRegenResult.spectral.spectral_centroid_hz)} Hz ·
                      Low {sourceRegenResult.spectral.low_band_pct.toFixed(0)}% / Mid{' '}
                      {sourceRegenResult.spectral.mid_band_pct.toFixed(0)}% / High{' '}
                      {sourceRegenResult.spectral.high_band_pct.toFixed(0)}%
                    </div>
                    <div className="text-[#79b89e]">
                      {sourceRegenResult.noteCount} cues · SHA{' '}
                      {sourceRegenResult.drySha256.slice(0, 12)} · 0% original blended
                    </div>
                  </div>
                )}
              </div>
            )}

            {/* Split Info Panel: Published Reference Data vs What Rust Actually Uses */}
            {activeSource && (
              <div className="border-t border-[#363636] pt-2 space-y-1.5 text-[10px]">
                <div className="bg-[#1e1e1e] border border-[#363636] p-2 space-y-1">
                  <div className="flex items-center justify-between gap-1">
                    <span className="uppercase tracking-wider text-[#8c8c8c] font-semibold">
                      Published Reference Data
                    </span>
                    <span className="text-[9px] text-[#a0a0a0] font-mono">Reference Only</span>
                  </div>
                  <div className="text-[#dedede] font-medium">
                    {activeSource.name}{' '}
                    <span className="text-[#969696] font-normal">({activeSource.family})</span>
                  </div>
                  <div className="grid grid-cols-2 gap-x-2 gap-y-0.5 font-mono text-[9.5px] text-[#b4b4b4]">
                    <div>
                      <span className="text-[#7c7c7c]">Fund:</span>{' '}
                      {formatHzRange(activeSource.published_range_hz)}
                      {activeSource.published_fundamental_notes
                        ? ` (${activeSource.published_fundamental_notes})`
                        : ''}
                    </div>
                    <div>
                      <span className="text-[#7c7c7c]">Practical:</span>{' '}
                      {formatHzRange(activeSource.published_practical_range_hz)}
                    </div>
                  </div>
                  {/* Broad Spectral Character Regions */}
                  <div className="grid grid-cols-3 gap-1 pt-0.5 font-mono text-[9px]">
                    <div className="bg-[#252525] border border-[#353535] px-1.5 py-1">
                      <div className="text-[#888888] font-sans">Body / Warmth</div>
                      <div className="text-[#dcdcdc]">
                        {formatHzRange(activeSource.spectral_regions?.body_warmth_hz)}
                      </div>
                    </div>
                    <div className="bg-[#252525] border border-[#353535] px-1.5 py-1">
                      <div className="text-[#888888] font-sans">Presence / Atk</div>
                      <div className="text-[#dcdcdc]">
                        {formatHzRange(activeSource.spectral_regions?.presence_attack_hz)}
                      </div>
                    </div>
                    <div className="bg-[#252525] border border-[#353535] px-1.5 py-1">
                      <div className="text-[#888888] font-sans">High / Air</div>
                      <div className="text-[#dcdcdc]">
                        {formatHzRange(activeSource.spectral_regions?.air_high_hz)}
                      </div>
                    </div>
                  </div>
                  <div className="text-[#9a9a9a] leading-snug">
                    {activeSource.spectral_regions?.formants_and_resonances_note ??
                      activeSource.published_notes}
                  </div>
                  {activeSource.source_references?.length > 0 && (
                    <div className="text-[9px] text-[#7c7c7c] leading-snug border-t border-[#2d2d2d] pt-1">
                      <strong className="text-[#969696]">Sources:</strong>{' '}
                      {activeSource.source_references.join(' · ')}
                    </div>
                  )}
                  {activeSource.uncertainties?.length > 0 && (
                    <div className="text-[9px] text-[#8a8068] leading-snug">
                      <strong className="text-[#a89b7c]">Uncertainties:</strong>{' '}
                      {activeSource.uncertainties[0]}
                    </div>
                  )}
                </div>

                <div className="bg-[#1d232b] border border-[#334152] p-2 space-y-1.5">
                  <div className="flex items-center justify-between gap-1">
                    <span className="uppercase tracking-wider text-[#7db4ee] font-semibold">
                      What The Rust Renderer Actually Uses
                    </span>
                    <span className="text-[9px] text-[#7db4ee] font-mono">Physics Constraint</span>
                  </div>
                  <div className="text-[#d4e2f0]">
                    Synthesis Model:{' '}
                    <strong>
                      {activeSource.regen_model?.model_label ?? 'Baseline Modal Cue Synth'}
                    </strong>{' '}
                    <span className="font-mono text-[9px] text-[#8ab4e0]">
                      ({activeSource.regen_model?.model_id ?? 'neutral_cue_modal'})
                    </span>
                    {activeSource.synthesis_model_hint && (
                      <span className="ml-1.5 px-1 py-0.2 bg-[#233245] border border-[#3a5574] font-mono text-[8.5px] text-[#9ecaff]">
                        {activeSource.synthesis_model_hint}
                      </span>
                    )}
                  </div>
                  <div className="text-[#a8bfd6] leading-snug">
                    {activeSource.regen_model?.description}
                  </div>

                  {activeSource.regen_model?.physics_constraint && (
                    <div className="bg-[#161c23] border border-[#2a3848] p-1.5 space-y-1 font-mono text-[9px]">
                      <div className="grid grid-cols-2 gap-1.5 text-[#c8daec]">
                        <div>
                          <span className="text-[#7c97b3]">Atk / Rel:</span>{' '}
                          {activeSource.regen_model.physics_constraint.attack_ms.toFixed(0)} ms /{' '}
                          {activeSource.regen_model.physics_constraint.release_ms.toFixed(0)} ms
                        </div>
                        <div>
                          <span className="text-[#7c97b3]">Ceiling:</span>{' '}
                          {activeSource.regen_model.physics_constraint.harmonic_ceiling_hz
                            ? `LP ${(activeSource.regen_model.physics_constraint.harmonic_ceiling_hz / 1000).toFixed(1)} kHz`
                            : 'Full cue bandwidth'}
                        </div>
                      </div>
                      <div className="text-[#b8cee3] leading-snug">
                        <span className="text-[#7c97b3]">Resonance:</span>{' '}
                        {activeSource.regen_model.physics_constraint.resonance_contour_summary}
                      </div>
                      <div className="text-[#b8cee3] leading-snug">
                        <span className="text-[#7c97b3]">Texture:</span>{' '}
                        {activeSource.regen_model.physics_constraint.texture_mechanism}
                      </div>
                      <div className="text-[#8cb2d6] leading-snug">
                        {activeSource.regen_model.physics_constraint.plausibility_note}
                      </div>
                    </div>
                  )}

                  <div className="text-[#d4e2f0] border-t border-[#2c3b4c] pt-1">
                    Spatial radiation: <strong>{activeSource.radiation_label}</strong> (α=
                    {activeSource.radiation_alpha.toFixed(2)})
                  </div>
                  <div className="bg-[#182026] border border-[#2a3b47] px-1.5 py-1 font-mono text-[9px] text-[#9ad9be] leading-snug">
                    Cue precedence: Cues drive pitch, bends, slides, timing, and atypical decay.
                    Published range is soft guidance only — never clamped, never invents uncertain
                    spans, 0% original audio blended.
                  </div>
                </div>
              </div>
            )}
          </>
        )}

        {/* ================= 2. MICROPHONE TAB ================= */}
        {activeTab === 'mic' && (
          <>
            {/* Multi-mic slot bar */}
            <div className="flex items-center justify-between gap-1">
              <div className="flex items-center gap-1 flex-wrap">
                {rig.mics.map((slot, idx) => (
                  <button
                    key={slot.id}
                    type="button"
                    onClick={() => onUpdateRig({ ...rig, selectedMic: idx }, false)}
                    className={`px-2 py-0.5 border text-[10px] ${
                      rig.selectedMic === idx
                        ? 'bg-[#378ef0] text-white border-[#378ef0]'
                        : 'bg-[#323232] text-[#bdbdbd] border-[#424242]'
                    }`}
                  >
                    M{idx + 1}
                  </button>
                ))}
              </div>
              <div className="flex items-center gap-1">
                <button
                  type="button"
                  disabled={rig.mics.length >= 4}
                  onClick={() => {
                    const nextSlot = makeDefaultMicSlot(rig.mics.length);
                    onUpdateRig(
                      clampRig({
                        ...rig,
                        mics: [...rig.mics, nextSlot],
                        selectedMic: rig.mics.length,
                      }),
                      true,
                    );
                  }}
                  className="p-1 bg-[#323232] border border-[#444444] text-[#d0d0d0] hover:bg-[#3c3c3c] disabled:opacity-35"
                  title="Add virtual mic slot"
                >
                  <Plus className="w-3 h-3" />
                </button>
                <button
                  type="button"
                  disabled={rig.mics.length <= 1}
                  onClick={() => {
                    const nextMics = rig.mics.filter((_, i) => i !== rig.selectedMic);
                    onUpdateRig(
                      {
                        ...rig,
                        mics: nextMics,
                        selectedMic: Math.min(rig.selectedMic, nextMics.length - 1),
                      },
                      true,
                    );
                  }}
                  className="p-1 bg-[#323232] border border-[#444444] text-[#d0d0d0] hover:bg-[#3c3c3c] disabled:opacity-35"
                  title="Remove selected mic slot"
                >
                  <Trash2 className="w-3 h-3" />
                </button>
              </div>
            </div>

            {/* Search + Brand -> Model picker (All 972 Mic Archive entries across 88 brands) */}
            <div className="space-y-1">
              <div className="flex items-center justify-between text-[10px]">
                <span className="uppercase tracking-wider text-[#909090]">
                  Brand → Model ({filteredMics.length} / {mics.length} Mic Archive)
                </span>
                <button
                  type="button"
                  onClick={() =>
                    updateActiveMic((m) => ({
                      ...m,
                      catalogIndex: 0,
                      patternOverride: null,
                      offsetM: { x: 0, y: 2 * FEET_TO_METERS, z: 0 },
                      aim: { tiltXDeg: 0, rollYDeg: 0, headingZDeg: 0, followSource: false },
                    }))
                  }
                  className="text-[10px] text-[#7db4ee] hover:underline"
                  title="Reset selected mic to SM7B guitar reference ~2 ft in front of source"
                >
                  SM7B 2 ft Ref
                </button>
              </div>
              <div className="grid grid-cols-[120px_1fr] gap-1">
                <select
                  aria-label="Microphone Brand"
                  value={micBrand}
                  onChange={(e) => setMicBrand(e.target.value)}
                  className="px-1.5 py-1 bg-[#1e1e1e] border border-[#3e3e3e] text-[#dcdcdc] text-[11px]"
                >
                  <option value="All">All Brands ({micBrands.length})</option>
                  {micBrands.map(({ brand, count }) => (
                    <option key={brand} value={brand}>
                      {brand} ({count})
                    </option>
                  ))}
                </select>
                <input
                  type="text"
                  value={micQuery}
                  onChange={(e) => setMicQuery(e.target.value)}
                  placeholder="Search 972 mics (MKH 50, DPA 4011A, SM7B)…"
                  className="px-2 py-1 bg-[#1e1e1e] border border-[#3e3e3e] text-[#dcdcdc] placeholder-[#707070]"
                />
              </div>
              <div className="max-h-28 overflow-y-auto border border-[#3a3a3a] bg-[#1e1e1e] divide-y divide-[#2a2a2a]">
                {filteredMics.map((m) => (
                  <button
                    key={m.id}
                    type="button"
                    // Swapping mic model NEVER moves position or aim (uses pure single-source helper)
                    onClick={() =>
                      onUpdateRig(setMicCatalogIndex(rig, rig.selectedMic, m.index), true)
                    }
                    className={`w-full text-left px-2 py-1 flex items-center justify-between gap-2 ${
                      activeSlot.catalogIndex === m.index
                        ? 'bg-[#378ef0] text-white'
                        : 'text-[#cccccc] hover:bg-[#2c2c2c]'
                    }`}
                  >
                    <span className="truncate">
                      {m.brand} {m.model}
                    </span>
                    <span className="font-mono text-[9px] opacity-75 shrink-0">
                      {m.default_pattern_label}
                    </span>
                  </button>
                ))}
              </div>
            </div>

            {/* Independent Pickup Pattern + Compact Polar Chart beside Mic Thumbnail */}
            <div className="flex items-center gap-2 border-t border-[#363636] pt-2">
              <MicThumbnailSvg />
              <MiniPolarSvg pattern={activePattern} />
              <div className="flex-1 space-y-1 min-w-0">
                <div className="flex items-center justify-between text-[10px]">
                  <span className="uppercase tracking-wider text-[#909090]">Pickup Pattern</span>
                  {activeSlot.patternOverride && (
                    <button
                      type="button"
                      onClick={() => updateActiveMic((s) => ({ ...s, patternOverride: null }))}
                      className="text-[9px] text-[#7db4ee] hover:underline"
                    >
                      Default
                    </button>
                  )}
                </div>
                <select
                  aria-label="Pickup Pattern"
                  value={activePattern}
                  onChange={(e) =>
                    updateActiveMic((s) => ({
                      ...s,
                      patternOverride: e.target.value as PolarPattern,
                    }))
                  }
                  className="w-full px-1.5 py-1 bg-[#1e1e1e] border border-[#3e3e3e] text-[#e0e0e0]"
                >
                  {ALL_PATTERNS.map((p) => (
                    <option key={p.key} value={p.key}>
                      {p.label}
                    </option>
                  ))}
                </select>
                <div className="font-mono text-[9px] text-[#8d8d8d] truncate">
                  Dist: {metersToDisplay(distM, rig.unit).toFixed(dec)} {unitSuffix} ·{' '}
                  {activeSlot.patternOverride ? 'Independent override' : 'Catalog default'}
                </div>
              </div>
            </div>

            {/* Position (X, Y, Z) — moving mic never silently rotates it unless Follow Source is on */}
            <div className="space-y-1 border-t border-[#363636] pt-2">
              <div className="text-[10px] uppercase tracking-wider text-[#909090]">
                Mic Position ({unitSuffix}) · Independent of Aim
              </div>
              <div className="grid grid-cols-3 gap-1.5">
                {(['x', 'y', 'z'] as const).map((axis) => (
                  <div key={axis} className="flex flex-col gap-0.5">
                    <div className="flex items-center justify-between text-[10px] text-[#8d8d8d]">
                      <span className="uppercase">{axis}</span>
                      <span className="flex gap-0.5">
                        <button
                          type="button"
                          onClick={() => nudgeMic(axis, -0.5)}
                          className="px-1 bg-[#2e2e2e] hover:bg-[#3a3a3a]"
                        >
                          −
                        </button>
                        <button
                          type="button"
                          onClick={() => nudgeMic(axis, 0.5)}
                          className="px-1 bg-[#2e2e2e] hover:bg-[#3a3a3a]"
                        >
                          +
                        </button>
                      </span>
                    </div>
                    <input
                      type="number"
                      step={0.1}
                      aria-label={`Mic ${axis.toUpperCase()}`}
                      value={Number(metersToDisplay(micPos[axis], rig.unit).toFixed(dec))}
                      onChange={(e) => {
                        const m = displayToMeters(Number(e.target.value) || 0, rig.unit);
                        onUpdateRig(
                          setMicWorldPosM(rig, rig.selectedMic, { ...micPos, [axis]: m }),
                          true,
                        );
                      }}
                      className="w-full px-1.5 py-1 bg-[#1e1e1e] border border-[#3e3e3e] font-mono tabular-nums text-[#e0e0e0]"
                    />
                  </div>
                ))}
              </div>
            </div>

            {/* Aiming: Rotation Ball (8° snap with green pulse) + X tilt / Y roll / Z heading + Aim menu */}
            <div className="space-y-1.5 border-t border-[#363636] pt-2">
              <div className="flex items-center justify-between">
                <span className="text-[10px] uppercase tracking-wider text-[#909090]">
                  Aim (Never Moves Mic)
                </span>
                <label className="flex items-center gap-1 text-[10px] text-[#c4c4c4] cursor-pointer">
                  <input
                    type="checkbox"
                    checked={activeSlot.aim.followSource}
                    onChange={(e) => {
                      const follow = e.target.checked;
                      updateActiveMic((s) => {
                        const p = follow ? anglesPointingAtSource(s.offsetM) : null;
                        return {
                          ...s,
                          aim: {
                            ...s.aim,
                            followSource: follow,
                            ...(p ? { headingZDeg: p.headingZDeg, tiltXDeg: p.tiltXDeg } : {}),
                          },
                        };
                      });
                    }}
                    className="accent-[#378ef0] w-3 h-3"
                  />
                  <span>Follow Source</span>
                </label>
              </div>

              <div className="flex items-center gap-2.5">
                <RotationBall
                  aim={activeSlot.aim}
                  pulseAxis={snapPulseAxis}
                  onDrag={(headingZDeg, tiltXDeg) =>
                    updateActiveMic(
                      (s) => ({
                        ...s,
                        aim: { ...s.aim, headingZDeg, tiltXDeg, followSource: false },
                      }),
                      false,
                    )
                  }
                  onRelease={(headingZDeg, tiltXDeg) => {
                    const res = snapAnglesToCardinalWithinThreshold(
                      { ...activeSlot.aim, headingZDeg, tiltXDeg, followSource: false },
                      8,
                    );
                    if (res.snapped && res.axis) {
                      setSnapPulseAxis(res.axis);
                      window.setTimeout(() => setSnapPulseAxis(null), 550);
                    }
                    updateActiveMic((s) => ({ ...s, aim: res.aim }), true);
                  }}
                />
                <div className="flex-1 space-y-1">
                  <div className="grid grid-cols-3 gap-1">
                    {(
                      [
                        ['tiltXDeg', 'X tilt', -90, 90],
                        ['rollYDeg', 'Y roll', -180, 180],
                        ['headingZDeg', 'Z head', -180, 180],
                      ] as const
                    ).map(([field, lbl, min, max]) => (
                      <label key={field} className="flex flex-col gap-0.5">
                        <span className="text-[9px] text-[#8d8d8d]">{lbl}</span>
                        <input
                          type="number"
                          step={5}
                          min={min}
                          max={max}
                          value={Math.round(activeSlot.aim[field])}
                          onChange={(e) => {
                            const v = Math.max(min, Math.min(max, Number(e.target.value) || 0));
                            updateActiveMic((s) => ({
                              ...s,
                              aim: { ...s.aim, [field]: v, followSource: false },
                            }));
                          }}
                          className="w-full px-1 py-0.5 bg-[#1e1e1e] border border-[#3e3e3e] font-mono tabular-nums text-[10px] text-[#e0e0e0]"
                        />
                      </label>
                    ))}
                  </div>
                  <div className="flex items-center gap-1">
                    <button
                      type="button"
                      onClick={() =>
                        updateActiveMic((s) => {
                          const p = anglesPointingAtSource(s.offsetM);
                          return {
                            ...s,
                            aim: { ...s.aim, headingZDeg: p.headingZDeg, tiltXDeg: p.tiltXDeg },
                          };
                        })
                      }
                      className="flex-1 py-0.5 text-[10px] bg-[#323232] border border-[#444444] text-[#d4d4d4] hover:bg-[#3c3c3c]"
                    >
                      Aim at Source
                    </button>
                    <button
                      type="button"
                      onClick={() =>
                        updateActiveMic((s) => ({
                          ...s,
                          aim: {
                            ...s.aim,
                            tiltXDeg: Math.round(s.aim.tiltXDeg / 90) * 90,
                            rollYDeg: Math.round(s.aim.rollYDeg / 90) * 90,
                            headingZDeg: Math.round(s.aim.headingZDeg / 90) * 90,
                            followSource: false,
                          },
                        }))
                      }
                      className="px-1.5 py-0.5 text-[10px] bg-[#323232] border border-[#444444] text-[#c8c8c8] hover:bg-[#3c3c3c]"
                      title="Snap angles to nearest 90°"
                    >
                      90°
                    </button>
                  </div>
                  <div className="grid grid-cols-6 gap-0.5">
                    {GRID_DIRS.map((dir) => (
                      <button
                        key={dir}
                        type="button"
                        onClick={() => {
                          const a = gridDirectionAngles(dir);
                          updateActiveMic((s) => ({
                            ...s,
                            aim: { ...a, followSource: false },
                          }));
                        }}
                        className="py-0.5 font-mono text-[9px] bg-[#2e2e2e] border border-[#3e3e3e] text-[#c2c2c2] hover:bg-[#3a3a3a]"
                      >
                        {dir}
                      </button>
                    ))}
                  </div>
                </div>
              </div>
            </div>

            {/* Direct Path Aim & Polar Weighting (Renderer Trigonometry — Informational) */}
            <div className="border-t border-[#363636] pt-2 space-y-1.5 text-[10px]">
              <div className="bg-[#1b222a] border border-[#314154] p-2 space-y-1">
                <div className="flex items-center justify-between gap-1">
                  <span className="uppercase tracking-wider text-[#7db4ee] font-semibold">
                    Direct Path &amp; Polar Weighting
                  </span>
                  <span className="font-mono text-[8.5px] text-[#7db4ee]">
                    {analysisFromRust ? 'Rust Math' : 'Trig'}
                  </span>
                </div>
                <div className="grid grid-cols-2 gap-x-2 gap-y-0.5 font-mono text-[9.5px] text-[#c8d8e8]">
                  <div>
                    Dist:{' '}
                    <strong>
                      {metersToDisplay(analysis.direct_path.distance_m, rig.unit).toFixed(dec)}{' '}
                      {unitSuffix}
                    </strong>
                  </div>
                  <div>
                    Delay: <strong>{analysis.direct_path.direct_delay_ms.toFixed(3)} ms</strong>
                  </div>
                  <div>
                    Inv-Sq (vs 1m):{' '}
                    <strong>
                      {analysis.direct_path.inverse_square_db_vs_1m >= 0 ? '+' : ''}
                      {analysis.direct_path.inverse_square_db_vs_1m.toFixed(1)} dB
                    </strong>
                  </div>
                  <div>
                    Off-axis θ: <strong>{analysis.direct_path.off_axis_deg.toFixed(1)}°</strong>
                  </div>
                  <div>
                    cos(θ): <strong>{analysis.direct_path.cos_theta.toFixed(3)}</strong>
                  </div>
                  <div>
                    Polar g:{' '}
                    <strong>
                      {analysis.direct_path.polar_gain_linear.toFixed(3)}
                      {analysis.direct_path.polar_gain_db !== null
                        ? ` (${analysis.direct_path.polar_gain_db.toFixed(1)} dB)`
                        : ' (-∞ dB)'}
                    </strong>
                  </div>
                </div>
                <div className="font-mono text-[9px] text-[#8ca6c0]">
                  {analysis.direct_path.pattern_label}: {analysis.direct_path.pattern_formula} · c ={' '}
                  {analysis.speed_of_sound_mps.toFixed(1)} m/s
                </div>
              </div>

              {/* Multi-Mic Placement Guidance (3:1 Rule & Stereo Pairs — Guidance Only) */}
              <div className="bg-[#1e1e1e] border border-[#3a3a3a] p-2 space-y-1.5">
                <div className="flex items-center justify-between gap-1">
                  <span className="uppercase tracking-wider text-[#b4b4b4] font-semibold">
                    Placement Guidance · 3:1 &amp; Stereo Pairs
                  </span>
                  <span className="font-mono text-[8.5px] text-[#9a9a9a]">Guidance Only</span>
                </div>

                {/* Stereo Quick-Apply Presets (Moves M1 & M2 cleanly, updates capture) */}
                <div className="space-y-1">
                  <div className="text-[9px] text-[#8d8d8d]">
                    Quick-apply stereo pair (sets M1 &amp; M2 position &amp; aim):
                  </div>
                  <div className="grid grid-cols-3 gap-1">
                    <button
                      type="button"
                      onClick={() => onUpdateRig(applyStereoPreset(rig, 'ortf'), true)}
                      className="py-1 px-1 text-[9.5px] bg-[#2c333c] border border-[#425266] text-[#d0e2f5] hover:bg-[#35404d]"
                      title="ORTF: 17 cm spacing, 110° included angle (±55°), cardioid capsules"
                    >
                      ORTF 17cm
                    </button>
                    <button
                      type="button"
                      onClick={() => onUpdateRig(applyStereoPreset(rig, 'xy', 90), true)}
                      className="py-1 px-1 text-[9.5px] bg-[#2c333c] border border-[#425266] text-[#d0e2f5] hover:bg-[#35404d]"
                      title="X/Y: coincident cardioid capsules angled 90° (±45°)"
                    >
                      X/Y 90°
                    </button>
                    <button
                      type="button"
                      onClick={() => onUpdateRig(applyStereoPreset(rig, 'spaced', 60), true)}
                      className="py-1 px-1 text-[9.5px] bg-[#2c333c] border border-[#425266] text-[#d0e2f5] hover:bg-[#35404d]"
                      title="Spaced pair: d × tan(width/2) omni pair aimed at source"
                    >
                      Spaced 60°
                    </button>
                  </div>
                </div>

                {analysis.placement_guidance.multi_mic_active ? (
                  <div className="space-y-1 border-t border-[#303030] pt-1.5">
                    <div className="flex items-center justify-between font-mono text-[9px] text-[#c8c8c8]">
                      <span>
                        3:1 min:{' '}
                        <strong className="text-[#7db4ee]">
                          {metersToDisplay(
                            analysis.placement_guidance.three_to_one_min_distance_m,
                            rig.unit,
                          ).toFixed(dec)}{' '}
                          {unitSuffix}
                        </strong>{' '}
                        (≈{analysis.placement_guidance.three_to_one_reference_level_diff_db.toFixed(1)} dB)
                      </span>
                      <label
                        className="flex items-center gap-1 text-[9px] text-[#bcbcbc] cursor-pointer"
                        title="Display-only 3:1 radius circle in Top & 3D views (never changes audio)"
                      >
                        <input
                          type="checkbox"
                          checked={rig.showGuidanceOverlay}
                          onChange={(e) =>
                            onUpdateRig({ ...rig, showGuidanceOverlay: e.target.checked }, false)
                          }
                          className="accent-[#378ef0] w-3 h-3"
                        />
                        <span>3:1 Ring</span>
                      </label>
                    </div>
                    <div className="space-y-0.5 font-mono text-[9px]">
                      {analysis.placement_guidance.slots.map((row) => (
                        <div
                          key={row.slot_index}
                          className="flex items-center justify-between gap-1 bg-[#242424] border border-[#323232] px-1.5 py-0.5"
                        >
                          <span className="truncate text-[#d4d4d4]">
                            M{row.slot_index + 1}:{' '}
                            {metersToDisplay(row.source_distance_m, rig.unit).toFixed(dec)}{' '}
                            {unitSuffix} · {row.direct_delay_ms.toFixed(2)}ms
                          </span>
                          <span
                            className={`shrink-0 ${
                              row.meets_three_to_one === true
                                ? 'text-[#52d18a]'
                                : row.meets_three_to_one === false
                                  ? 'text-[#f0b454]'
                                  : 'text-[#8ea8c3]'
                            }`}
                          >
                            {row.meets_three_to_one === false
                              ? `-${metersToDisplay(row.shortfall_m, rig.unit).toFixed(dec)} ${unitSuffix}`
                              : row.status_label}
                          </span>
                        </div>
                      ))}
                    </div>
                  </div>
                ) : (
                  <div className="text-[9px] text-[#858585] border-t border-[#2e2e2e] pt-1">
                    Single mic active ({metersToDisplay(analysis.direct_path.distance_m, rig.unit).toFixed(dec)}{' '}
                    {unitSuffix} · {analysis.direct_path.direct_delay_ms.toFixed(3)} ms). 3:1
                    guidance activates automatically with 2+ active mics.
                  </div>
                )}
              </div>
            </div>

            {/* Split Info Panel: Published Manufacturer / Mic Archive Reference vs Renderer Analytical Model */}
            {activeMicEntry && (
              <div className="border-t border-[#363636] pt-2 space-y-1.5 text-[10px]">
                {/* 1. Published Manufacturer / Mic Archive Reference Card */}
                <div className="bg-[#1e1e1e] border border-[#363636] p-2 space-y-1.5">
                  <div className="flex items-center justify-between gap-1">
                    <span className="uppercase tracking-wider text-[#909090] font-semibold">
                      Published Mic Archive Reference
                    </span>
                    <span className="text-[9px] font-mono text-[#a6a6a6]">
                      #{activeMicEntry.index + 1} / {mics.length}
                    </span>
                  </div>
                  <div className="text-[#dedede] font-medium">
                    {activeMicEntry.brand} {activeMicEntry.model}
                    <span className="text-[#9a9a9a] font-normal">
                      {' '}
                      · {activeMicEntry.published_type ?? activeMicEntry.transducer}
                    </span>
                  </div>

                  {/* Structured Manufacturer Spec Grid (Missing fields explicitly labeled, never invented) */}
                  <div className="grid grid-cols-2 gap-x-2 gap-y-0.5 font-mono text-[9.5px] text-[#b8b8b8] bg-[#242424] border border-[#323232] p-1.5">
                    <div>
                      <span className="text-[#7d7d7d]">Pattern(s):</span>{' '}
                      {activeMicEntry.published_patterns ?? 'Not listed'}
                    </div>
                    <div>
                      <span className="text-[#7d7d7d]">Range:</span>{' '}
                      {activeMicEntry.published_range_text ??
                        (activeMicEntry.published_range_hz
                          ? `${activeMicEntry.published_range_hz[0]}–${activeMicEntry.published_range_hz[1]} Hz`
                          : 'Not listed')}
                    </div>
                    <div>
                      <span className="text-[#7d7d7d]">Sens:</span>{' '}
                      {activeMicEntry.published_sensitivity_dbv !== null
                        ? `${activeMicEntry.published_sensitivity_dbv.toFixed(1)} dBV/Pa`
                        : 'Not listed'}
                    </div>
                    <div>
                      <span className="text-[#7d7d7d]">Self-Noise:</span>{' '}
                      {activeMicEntry.published_self_noise_dba !== null
                        ? `${activeMicEntry.published_self_noise_dba.toFixed(1)} dB(A)`
                        : 'Not listed'}
                    </div>
                    <div>
                      <span className="text-[#7d7d7d]">Max SPL:</span>{' '}
                      {activeMicEntry.published_max_spl_db !== null
                        ? `${activeMicEntry.published_max_spl_db.toFixed(0)} dB`
                        : 'Not listed'}
                    </div>
                    <div>
                      <span className="text-[#7d7d7d]">Imped:</span>{' '}
                      {activeMicEntry.published_impedance_ohm !== null
                        ? `${Math.round(activeMicEntry.published_impedance_ohm)} Ω`
                        : 'Not listed'}
                    </div>
                    <div>
                      <span className="text-[#7d7d7d]">Released:</span>{' '}
                      {activeMicEntry.published_released_year ?? 'Not listed'}
                    </div>
                    <div>
                      <span className="text-[#7d7d7d]">Power:</span>{' '}
                      {activeMicEntry.published_power ?? 'Not listed'}
                    </div>
                  </div>

                  {/* Inline SVG Published On-Axis Frequency Response Plot (Reference Only) */}
                  {activeMicEntry.published_fr_points && activeMicEntry.published_fr_points.length >= 2 ? (
                    <div className="bg-[#181818] border border-[#323232] p-1.5 space-y-1">
                      <div className="flex items-center justify-between text-[9px] text-[#8f8f8f]">
                        <span>
                          Published FR curve ({activeMicEntry.published_primary_curve ?? '0°'})
                        </span>
                        <span className="font-mono text-[#7c7c7c]">Reference only</span>
                      </div>
                      <svg
                        viewBox="0 0 240 68"
                        className="w-full h-[68px] block"
                        aria-label={`Published frequency response curve for ${activeMicEntry.brand} ${activeMicEntry.model}`}
                      >
                        {/* Horizontal 0 dB & ±10 dB grid lines */}
                        {[-10, 0, 10].map((db) => {
                          const y = 34 - (db / 20) * 26;
                          return (
                            <g key={db}>
                              <line
                                x1={22}
                                x2={234}
                                y1={y}
                                y2={y}
                                stroke={db === 0 ? '#474747' : '#2a2a2a'}
                                strokeDasharray={db === 0 ? undefined : '2 2'}
                              />
                              <text
                                x={19}
                                y={y + 3}
                                textAnchor="end"
                                fill="#727272"
                                fontSize="7"
                                fontFamily="monospace"
                              >
                                {db > 0 ? `+${db}` : db}
                              </text>
                            </g>
                          );
                        })}
                        {/* Log-frequency grid ticks */}
                        {[50, 100, 1000, 10000].map((hz) => {
                          const x =
                            22 + ((Math.log10(hz) - Math.log10(20)) / (Math.log10(20000) - Math.log10(20))) * 212;
                          return (
                            <g key={hz}>
                              <line x1={x} x2={x} y1={4} y2={58} stroke="#282828" />
                              <text
                                x={x}
                                y={66}
                                textAnchor="middle"
                                fill="#727272"
                                fontSize="7"
                                fontFamily="monospace"
                              >
                                {hz >= 1000 ? `${hz / 1000}k` : hz}
                              </text>
                            </g>
                          );
                        })}
                        <polyline
                          fill="none"
                          stroke="#6cd4b4"
                          strokeWidth="1.5"
                          points={activeMicEntry.published_fr_points
                            .map(([hz, db]) => {
                              const clampedHz = Math.max(20, Math.min(20000, hz));
                              const x =
                                22 +
                                ((Math.log10(clampedHz) - Math.log10(20)) /
                                  (Math.log10(20000) - Math.log10(20))) *
                                  212;
                              const clampedDb = Math.max(-20, Math.min(20, db));
                              const y = 34 - (clampedDb / 20) * 26;
                              return `${x.toFixed(1)},${y.toFixed(1)}`;
                            })
                            .join(' ')}
                        />
                      </svg>
                    </div>
                  ) : (
                    <div className="text-[9.5px] text-[#8a8068] bg-[#22201b] border border-[#3a3428] px-1.5 py-1">
                      No digitized frequency-response curve in Mic Archive for this entry (not invented).
                    </div>
                  )}

                  {/* Published Off-Axis Polar Attenuation Table (90° & 180° dB) */}
                  {activeMicEntry.published_offaxis &&
                    activeMicEntry.published_offaxis.freqs_hz.length > 0 && (
                      <div className="bg-[#181818] border border-[#323232] p-1.5 font-mono text-[9px] space-y-0.5">
                        <div className="font-sans text-[#8f8f8f] flex justify-between">
                          <span>Published Polar Off-Axis Attenuation (dB)</span>
                          <span className="font-mono text-[#7a7a7a]">Spec sheet</span>
                        </div>
                        <div className="grid grid-cols-[36px_repeat(6,minmax(0,1fr))] gap-0.5 text-[#7e7e7e]">
                          <span>Angle</span>
                          {activeMicEntry.published_offaxis.freqs_hz.slice(0, 6).map((hz, i) => (
                            <span key={i} className="text-right">
                              {hz >= 1000 ? `${hz / 1000}k` : hz}
                            </span>
                          ))}
                        </div>
                        <div className="grid grid-cols-[36px_repeat(6,minmax(0,1fr))] gap-0.5 text-[#cfcfcf]">
                          <span className="text-[#8e8e8e]">90°</span>
                          {activeMicEntry.published_offaxis.atten_90deg_db.slice(0, 6).map((v, i) => (
                            <span key={i} className="text-right">
                              {Number.isFinite(v) ? v.toFixed(0) : '—'}
                            </span>
                          ))}
                        </div>
                        <div className="grid grid-cols-[36px_repeat(6,minmax(0,1fr))] gap-0.5 text-[#a8c5e5]">
                          <span className="text-[#8e8e8e]">180°</span>
                          {activeMicEntry.published_offaxis.atten_180deg_db
                            .slice(0, 6)
                            .map((v, i) => (
                              <span key={i} className="text-right">
                                {Number.isFinite(v) ? v.toFixed(0) : '—'}
                              </span>
                            ))}
                        </div>
                      </div>
                    )}

                  <div className="text-[9px] text-[#848484] leading-snug">
                    {activeMicEntry.published_notes}
                    {activeMicEntry.published_curves?.length > 1 && (
                      <span className="text-[#787878]">
                        {' '}
                        · Curves: {activeMicEntry.published_curves.join(', ')}
                      </span>
                    )}
                  </div>
                  <div className="text-[9px] font-mono text-[#6f859c] truncate">
                    Ref: {activeMicEntry.source_url}
                  </div>
                  {activeMicEntry.uncertainties?.length > 0 && (
                    <div className="text-[9px] text-[#8a8068] leading-snug border-t border-[#2c2c2c] pt-1">
                      <strong className="text-[#a89b7c]">Missing / Uncertainties:</strong>{' '}
                      {activeMicEntry.uncertainties.slice(0, 2).join(' · ')}
                    </div>
                  )}
                </div>

                {/* 2. Separate Renderer-Actual Card */}
                <div className="bg-[#1d232b] border border-[#334152] p-2 space-y-1">
                  <div className="flex items-center justify-between gap-1">
                    <span className="uppercase tracking-wider text-[#7db4ee] font-semibold">
                      What The Rust Renderer Actually Uses
                    </span>
                    <span className="text-[9px] font-mono text-[#7db4ee]">Modeled DSP</span>
                  </div>
                  <div className="text-[#d4e2f0]">
                    Polar:{' '}
                    <strong>{ALL_PATTERNS.find((p) => p.key === activePattern)?.label}</strong>{' '}
                    <span className="text-[#8ab4e0] font-mono text-[9px]">
                      ({activeSlot.patternOverride ? 'User override' : 'Catalog primary'} · p(θ)=a+(1−a)cos θ)
                    </span>
                  </div>
                  <div className="text-[#c5d7ea]">
                    Profile: <span className="font-mono">{activeMicEntry.renderer_profile_id}</span>
                  </div>
                  <div className="text-[#9cb1c7] leading-snug">
                    {activeMicEntry.renderer_profile_description} Published spec-sheet curves and
                    polar plots are reference only — never treated as measured microphone emulations.
                  </div>
                </div>
              </div>
            )}
          </>
        )}

        {/* ================= 3. MATERIALS TAB ================= */}
        {activeTab === 'materials' && (
          <>
            {rig.preset === 'open' && (
              <div className="px-2 py-1.5 bg-[#242c24] border border-[#3d573d] text-[#9ed69e] text-[10px]">
                Open room: only <strong>Floor / Ground</strong> material applies. Walls & ceiling are
                open air (zero reflections).
              </div>
            )}
            {rig.preset === 'direct' && (
              <div className="px-2 py-1.5 bg-[#2a251c] border border-[#52452c] text-[#e5b96c] text-[10px]">
                Direct mode disables all room reflections. Choose Small / Medium / Large / Open to
                apply surface materials.
              </div>
            )}

            {/* 6 independent face buttons (Floor/Ceiling strictly independent!) */}
            <div className="space-y-1">
              <div className="text-[10px] uppercase tracking-wider text-[#909090]">
                Room Surfaces (6 Independent Faces)
              </div>
              <div className="grid grid-cols-2 gap-1">
                {faceLabels.map((lbl, faceIdx) => {
                  const disabled = rig.preset === 'open' && faceIdx !== 4;
                  const surfName = surfaces[rig.surfaceFaces[faceIdx]]?.name ?? '—';
                  return (
                    <button
                      key={lbl}
                      type="button"
                      disabled={disabled}
                      onClick={() => setSelectedFace(faceIdx)}
                      className={`px-1.5 py-1 text-left border text-[10px] disabled:opacity-35 ${
                        effectiveFace === faceIdx
                          ? 'bg-[#378ef0] text-white border-[#378ef0]'
                          : 'bg-[#2e2e2e] text-[#cccccc] border-[#3f3f3f] hover:bg-[#383838]'
                      }`}
                    >
                      <div className="font-medium truncate">{lbl}</div>
                      <div className="text-[9px] opacity-80 truncate">
                        {disabled ? 'Open air (inactive)' : surfName}
                      </div>
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Search across 32 real Rust materials */}
            <div className="space-y-1 border-t border-[#363636] pt-2">
              <div className="flex items-center justify-between text-[10px]">
                <span className="uppercase tracking-wider text-[#909090]">
                  Material Catalog ({surfaces.length} Rust profiles)
                </span>
                {rig.preset !== 'open' && effectiveFace < 4 && (
                  <button
                    type="button"
                    onClick={() => {
                      const chosen = rig.surfaceFaces[effectiveFace];
                      const nextFaces: [number, number, number, number, number, number] = [
                        chosen,
                        chosen,
                        chosen,
                        chosen,
                        rig.surfaceFaces[4],
                        rig.surfaceFaces[5],
                      ];
                      onUpdateRig({ ...rig, surfaceFaces: nextFaces }, true);
                    }}
                    className="text-[10px] text-[#7db4ee] hover:underline"
                    title="Copy selected wall material to all 4 walls without touching Floor or Ceiling"
                  >
                    Copy to 4 Walls
                  </button>
                )}
              </div>
              <input
                type="text"
                value={materialQuery}
                onChange={(e) => setMaterialQuery(e.target.value)}
                placeholder="Search 32 acoustic materials…"
                className="w-full px-2 py-1 bg-[#1e1e1e] border border-[#3e3e3e] text-[#dcdcdc] placeholder-[#707070]"
              />
              <div className="max-h-28 overflow-y-auto border border-[#3a3a3a] bg-[#1e1e1e] divide-y divide-[#2a2a2a]">
                {filteredSurfaces.map((mat) => (
                  <button
                    key={mat.id}
                    type="button"
                    onClick={() => {
                      const nextFaces = [...rig.surfaceFaces] as [
                        number,
                        number,
                        number,
                        number,
                        number,
                        number,
                      ];
                      nextFaces[effectiveFace] = mat.index;
                      onUpdateRig({ ...rig, surfaceFaces: nextFaces }, true);
                    }}
                    className={`w-full text-left px-2 py-1 flex items-center justify-between gap-1 ${
                      rig.surfaceFaces[effectiveFace] === mat.index
                        ? 'bg-[#378ef0] text-white'
                        : 'text-[#cccccc] hover:bg-[#2c2c2c]'
                    }`}
                  >
                    <span className="truncate">{mat.name}</span>
                    <span className="font-mono text-[9px] opacity-80 shrink-0">
                      R̄={mat.mean_reflectance.toFixed(2)}
                    </span>
                  </button>
                ))}
              </div>
            </div>

            {/* Published Material Reference vs Effective Renderer Values + Biot-JCA Slots */}
            {activeSurface && (
              <div className="border-t border-[#363636] pt-2 space-y-1.5 text-[10px]">
                {/* 1. Assembly, Standard & Response Kind */}
                <div className="bg-[#1e1e1e] border border-[#363636] p-2 space-y-1">
                  <div className="flex items-center justify-between gap-1">
                    <span className="text-[#dedede] font-medium truncate">
                      {faceLabels[effectiveFace]}: {activeSurface.name}
                    </span>
                    <span
                      className={`shrink-0 px-1 py-0.5 font-mono text-[8.5px] border ${
                        activeSurface.response_kind?.toLowerCase().includes('published')
                          ? 'bg-[#1d2a24] text-[#6cd4b4] border-[#2e4b3e]'
                          : 'bg-[#2a241a] text-[#e5b96c] border-[#4d402a]'
                      }`}
                    >
                      {activeSurface.response_kind ?? 'Published reference'}
                    </span>
                  </div>
                  <div className="grid grid-cols-2 gap-x-2 gap-y-0.5 font-mono text-[9.5px] text-[#b4b4b4]">
                    <div>
                      <span className="text-[#7c7c7c]">Type:</span> {activeSurface.material_type}
                    </div>
                    <div>
                      <span className="text-[#7c7c7c]">Thickness:</span>{' '}
                      {activeSurface.thickness_mm !== null
                        ? `${Number(activeSurface.thickness_mm.toFixed(1))} mm`
                        : 'Not specified'}
                    </div>
                    <div className="col-span-2">
                      <span className="text-[#7c7c7c]">Incidence:</span> {activeSurface.incidence}
                    </div>
                    <div className="col-span-2">
                      <span className="text-[#7c7c7c]">Standard:</span> {activeSurface.standard}
                    </div>
                    <div className="col-span-2 font-sans text-[#9a9a9a]">
                      <span className="text-[#7c7c7c]">Assembly:</span> {activeSurface.assembly}
                    </div>
                  </div>
                </div>

                {/* 2. 6-Band Raw Published vs Effective Clamped vs Passive Reflectance */}
                <div className="bg-[#1d232b] border border-[#334152] p-1.5 font-mono tabular-nums space-y-1">
                  <div className="flex items-center justify-between font-sans text-[9px] text-[#7db4ee]">
                    <span className="uppercase tracking-wider font-semibold">
                      6-Band Absorption (Raw Published vs Renderer Effective)
                    </span>
                  </div>
                  <div className="grid grid-cols-7 gap-0.5 text-[9px] text-[#858585]">
                    <span>Band</span>
                    {BANDS_HZ.map((hz) => (
                      <span key={hz} className="text-right">
                        {hz >= 1000 ? `${hz / 1000}k` : hz}
                      </span>
                    ))}
                  </div>
                  <div className="grid grid-cols-7 gap-0.5 text-[9px] text-[#cfcfcf]">
                    <span className="text-[#969696]">Raw α</span>
                    {activeSurface.raw_absorption_6band.map((v, i) => (
                      <span
                        key={i}
                        className={`text-right ${v > 1.0 ? 'text-[#e5b96c] font-semibold' : ''}`}
                        title={
                          v > 1.0
                            ? 'Raw reverberation-room Sabin value >1.00; clamped to 1.00 in passive renderer'
                            : undefined
                        }
                      >
                        {v.toFixed(2)}
                      </span>
                    ))}
                  </div>
                  <div className="grid grid-cols-7 gap-0.5 text-[9px] text-[#7db4ee]">
                    <span className="text-[#7db4ee]">Eff α</span>
                    {activeSurface.effective_absorption_6band.map((v, i) => (
                      <span key={i} className="text-right">
                        {v.toFixed(2)}
                      </span>
                    ))}
                  </div>
                  <div className="grid grid-cols-7 gap-0.5 text-[9px] text-[#6cd4b4]">
                    <span className="text-[#6cd4b4]">R=√1−α</span>
                    {activeSurface.pressure_reflectance_6band.map((v, i) => (
                      <span key={i} className="text-right">
                        {v.toFixed(2)}
                      </span>
                    ))}
                  </div>
                  <div className="font-sans text-[9px] text-[#8da4bc] leading-snug pt-0.5">
                    Reflected paths multiply R=√(1−α_eff) per bounce; direct path &amp; performed notes
                    are never altered by room materials.
                  </div>
                  {activeFaceContribution && (
                    <div className="border-t border-[#2b394a] pt-1 mt-1 grid grid-cols-2 gap-x-2 gap-y-0.5 text-[9px] text-[#a9c4de]">
                      <div>
                        Face Area: <strong>{activeFaceContribution.area_m2.toFixed(1)} m²</strong>
                      </div>
                      <div>
                        Face Sabins:{' '}
                        <strong>
                          {activeFaceContribution.active
                            ? `${activeFaceContribution.mean_sabins_m2.toFixed(1)} m² (${activeFaceContribution.share_of_total_absorption_pct.toFixed(0)}%)`
                            : 'Inactive'}
                        </strong>
                      </div>
                      <div>
                        Room Total:{' '}
                        <strong>{analysis.mean_total_absorption_sabins_m2.toFixed(1)} sabins</strong>
                      </div>
                      <div>
                        Est. RT60 (mid):{' '}
                        <strong>
                          {analysis.sabine_rt60_mid_s !== null
                            ? `${analysis.sabine_rt60_mid_s.toFixed(2)} s`
                            : 'N/A'}
                        </strong>
                      </div>
                    </div>
                  )}
                </div>

                {/* 3. Biot-JCA Porous Parameters (Alfa Acoustics / ASTM E1050 / ISO 10534-2 — Unfabricated) */}
                {activeSurface.biot && (
                  <div className="bg-[#1e1e1e] border border-[#363636] p-2 space-y-1">
                    <div className="flex items-center justify-between gap-1">
                      <span className="uppercase tracking-wider text-[#909090] font-semibold">
                        Biot-JCA Porous Parameters
                      </span>
                      <span className="font-mono text-[8.5px] text-[#888888]">
                        {activeSurface.biot.standard_ref}
                      </span>
                    </div>
                    <div className="grid grid-cols-2 gap-x-2 gap-y-0.5 font-mono text-[9px] text-[#9e9e9e]">
                      <div>
                        Porosity φ: {activeSurface.biot.porosity ?? 'Not published'}
                      </div>
                      <div>
                        Airflow σ: {activeSurface.biot.airflow_resistivity_pa_s_m2 ?? 'Not published'}
                      </div>
                      <div>
                        Tortuosity α∞: {activeSurface.biot.tortuosity ?? 'Not published'}
                      </div>
                      <div>VCL Λ: {activeSurface.biot.vcl_um ?? 'Not published'}</div>
                      <div className="col-span-2">
                        TCL Λ′: {activeSurface.biot.tcl_um ?? 'Not published'}
                      </div>
                    </div>
                    <div className="text-[9px] text-[#8a8068] leading-snug">
                      {activeSurface.biot.status}
                    </div>
                  </div>
                )}

                <div className="text-[#8e8e8e] leading-snug">{activeSurface.provenance}</div>
                {activeSurface.uncertainties?.length > 0 && (
                  <div className="text-[9px] text-[#7f7968] leading-snug">
                    {activeSurface.uncertainties[0]}
                  </div>
                )}
              </div>
            )}
          </>
        )}

        {/* ================= 4. ROOM SIZE TAB ================= */}
        {activeTab === 'room' && (
          <>
            {/* Preset Selector First */}
            <div className="space-y-1.5">
              <div className="text-[10px] uppercase tracking-wider text-[#909090]">
                Virtual Capture Room Preset
              </div>
              <div className="grid grid-cols-2 gap-1">
                {(
                  [
                    ['small', 'Small (10×10 ft)'],
                    ['medium', 'Medium (20×20 ft)'],
                    ['large', 'Large (50×50 ft)'],
                    ['open', 'Open (50×50×50 ft)'],
                  ] as [RoomPresetKey, string][]
                ).map(([key, label]) => (
                  <button
                    key={key}
                    type="button"
                    onClick={() => onUpdateRig(makeRoomPreset(key, rig), true)}
                    className={`py-1 px-1.5 border text-[10px] text-left ${
                      rig.preset === key
                        ? 'bg-[#378ef0] text-white border-[#378ef0]'
                        : 'bg-[#323232] text-[#c2c2c2] border-[#424242] hover:bg-[#3a3a3a]'
                    }`}
                  >
                    {label}
                  </button>
                ))}
              </div>
              <button
                type="button"
                onClick={() => onUpdateRig(makeRoomPreset('direct', rig), true)}
                className={`w-full py-1 px-2 border text-[10px] text-left ${
                  rig.preset === 'direct'
                    ? 'bg-[#378ef0] text-white border-[#378ef0]'
                    : 'bg-[#2c2c2c] text-[#a8a8a8] border-[#3d3d3d] hover:bg-[#363636]'
                }`}
              >
                Direct (No Added Room Reflections)
              </button>
            </div>

            {/* Length, Width, Height + Individual Chain Links + Link All / Unlink All */}
            <div className="space-y-1.5 border-t border-[#363636] pt-2">
              <div className="flex items-center justify-between">
                <span className="text-[10px] uppercase tracking-wider text-[#909090]">
                  Dimensions ({unitSuffix})
                </span>
                <div className="flex items-center gap-1">
                  <button
                    type="button"
                    onClick={() => onUpdateRig({ ...rig, dimLinked: [true, true, true] }, false)}
                    className="px-1.5 py-0.5 text-[9px] bg-[#323232] border border-[#444444] text-[#cccccc] hover:bg-[#3c3c3c]"
                  >
                    Link All
                  </button>
                  <button
                    type="button"
                    onClick={() => onUpdateRig({ ...rig, dimLinked: [false, false, false] }, false)}
                    className="px-1.5 py-0.5 text-[9px] bg-[#323232] border border-[#444444] text-[#cccccc] hover:bg-[#3c3c3c]"
                  >
                    Unlink All
                  </button>
                </div>
              </div>

              {(
                [
                  ['y', 1, 'Length (Y)'],
                  ['x', 0, 'Width (X)'],
                  ['z', 2, 'Height (Z)'],
                ] as const
              ).map(([axis, linkIdx, label]) => {
                const linked = rig.dimLinked[linkIdx];
                return (
                  <div key={axis} className="flex items-center gap-1.5">
                    <button
                      type="button"
                      onClick={() => {
                        const nextLinks = [...rig.dimLinked] as [boolean, boolean, boolean];
                        nextLinks[linkIdx] = !nextLinks[linkIdx];
                        onUpdateRig({ ...rig, dimLinked: nextLinks }, false);
                      }}
                      title={linked ? 'Axis linked — click to unlink' : 'Axis unlinked — click to link'}
                      className={`p-1 border ${
                        linked
                          ? 'bg-[#2a3f59] border-[#378ef0] text-[#7db4ee]'
                          : 'bg-[#2a2a2a] border-[#3e3e3e] text-[#777777]'
                      }`}
                    >
                      {linked ? <Link2 className="w-3 h-3" /> : <Unlink2 className="w-3 h-3" />}
                    </button>
                    <span className="w-20 text-[10px] text-[#b5b5b5]">{label}</span>
                    <input
                      type="number"
                      step={rig.unit === 'mm' ? 100 : rig.unit === 'cm' || rig.unit === 'in' ? 6 : 1}
                      aria-label={label}
                      value={Number(metersToDisplay(rig.roomMeters[axis], rig.unit).toFixed(dec))}
                      onChange={(e) => {
                        const v = Number(e.target.value);
                        if (!Number.isFinite(v) || v <= 0) return;
                        onUpdateRig(
                          setLinkedDimensionM(rig, axis, displayToMeters(v, rig.unit)),
                          true,
                        );
                      }}
                      className="flex-1 px-1.5 py-1 bg-[#1e1e1e] border border-[#3e3e3e] font-mono tabular-nums text-[#e0e0e0]"
                    />
                    <span className="w-6 text-[10px] font-mono text-[#888888]">{unitSuffix}</span>
                  </div>
                );
              })}
            </div>

            {/* Display Units: ft, in, yd, mm, cm, m (NEVER resizes physical room or alters audio) */}
            <div className="space-y-1 border-t border-[#363636] pt-2">
              <div className="flex items-center justify-between text-[10px]">
                <span className="uppercase tracking-wider text-[#909090]">Display Units</span>
                <span className="text-[9px] text-[#7f7f7f]">Display only · stored in m</span>
              </div>
              <div className="grid grid-cols-6 gap-1">
                {ALL_UNITS.map((u) => (
                  <button
                    key={u}
                    type="button"
                    onClick={() => onUpdateRig({ ...rig, unit: u }, false)}
                    className={`py-1 font-mono text-[10px] border ${
                      rig.unit === u
                        ? 'bg-[#378ef0] text-white border-[#378ef0]'
                        : 'bg-[#323232] text-[#bdbdbd] border-[#424242] hover:bg-[#3a3a3a]'
                    }`}
                  >
                    {u}
                  </button>
                ))}
              </div>
            </div>

            {/* Air Temp & Physical Readout */}
            <div className="grid grid-cols-2 gap-2 border-t border-[#363636] pt-2 items-center">
              <label className="flex flex-col gap-0.5">
                <span className="text-[10px] text-[#8d8d8d]">Air Temp (°C)</span>
                <input
                  type="number"
                  step={1}
                  min={-20}
                  max={50}
                  value={rig.temperatureC}
                  onChange={(e) =>
                    onUpdateRig(
                      { ...rig, temperatureC: Math.max(-20, Math.min(50, Number(e.target.value) || 20)) },
                      true,
                    )
                  }
                  className="w-full px-1.5 py-1 bg-[#1e1e1e] border border-[#3e3e3e] font-mono tabular-nums text-[#e0e0e0]"
                />
              </label>
              <div className="text-[10px] font-mono text-[#9e9e9e] space-y-0.5 pt-3">
                <div>c = {analysis.speed_of_sound_mps.toFixed(1)} m/s</div>
                <div>
                  src = {metersToDisplay(srcPos.x, rig.unit).toFixed(1)},{' '}
                  {metersToDisplay(srcPos.y, rig.unit).toFixed(1)},{' '}
                  {metersToDisplay(srcPos.z, rig.unit).toFixed(1)} {unitSuffix}
                </div>
              </div>
            </div>

            {/* Acoustic Calculators & Estimates (Sabine RT60, Axial Modes, Direct Delay — Informational Only) */}
            <div className="border-t border-[#363636] pt-2 space-y-1.5 text-[10px]">
              <div className="bg-[#1b222a] border border-[#314154] p-2 space-y-1.5">
                <div className="flex items-center justify-between gap-1">
                  <span className="uppercase tracking-wider text-[#7db4ee] font-semibold">
                    Room Acoustic Estimates
                  </span>
                  <span className="font-mono text-[8.5px] text-[#7db4ee]">
                    {analysisFromRust ? 'Estimate · Rust' : 'Estimate'}
                  </span>
                </div>

                {/* Direct Path Delay & Volume / Absorption Summary */}
                <div className="grid grid-cols-2 gap-x-2 gap-y-0.5 font-mono text-[9.5px] text-[#c8d8e8]">
                  <div>
                    Src→Mic:{' '}
                    <strong>
                      {metersToDisplay(analysis.direct_path.distance_m, rig.unit).toFixed(dec)}{' '}
                      {unitSuffix}
                    </strong>
                  </div>
                  <div>
                    Delay: <strong>{analysis.direct_path.direct_delay_ms.toFixed(3)} ms</strong>
                  </div>
                  <div>
                    Volume: <strong>{analysis.volume_m3.toFixed(1)} m³</strong>
                  </div>
                  <div>
                    Sabins: <strong>{analysis.mean_total_absorption_sabins_m2.toFixed(1)} m²</strong>
                  </div>
                  <div>
                    RT60 Mid (est):{' '}
                    <strong className="text-[#6cd4b4]">
                      {analysis.sabine_rt60_mid_s !== null
                        ? `${analysis.sabine_rt60_mid_s.toFixed(2)} s`
                        : 'N/A'}
                    </strong>
                  </div>
                  <div>
                    RT60 Avg (est):{' '}
                    <strong>
                      {analysis.sabine_rt60_overall_s !== null
                        ? `${analysis.sabine_rt60_overall_s.toFixed(2)} s`
                        : 'N/A'}
                    </strong>
                  </div>
                </div>

                {/* 6-Band Sabine RT60 Estimate Row */}
                {analysis.is_enclosed_room ? (
                  <div className="border-t border-[#2b394a] pt-1 font-mono">
                    <div className="grid grid-cols-7 gap-0.5 text-[8.5px] text-[#7c94ad]">
                      <span>Sabine</span>
                      {BANDS_HZ.map((b) => (
                        <span key={b} className="text-right">
                          {b >= 1000 ? `${b / 1000}k` : b}
                        </span>
                      ))}
                    </div>
                    <div className="grid grid-cols-7 gap-0.5 text-[9px] text-[#6cd4b4]">
                      <span className="text-[#8cb2d9]">RT60</span>
                      {analysis.sabine_rt60_6band_s.map((v, i) => (
                        <span key={i} className="text-right">
                          {v !== null ? `${v.toFixed(2)}` : '—'}
                        </span>
                      ))}
                    </div>
                  </div>
                ) : (
                  <div className="text-[9px] text-[#8da4bc] border-t border-[#2b394a] pt-1">
                    Sabine RT60 &amp; axial room modes are N/A in{' '}
                    <strong>{rig.preset === 'open' ? 'Open Field' : 'Direct'}</strong> mode (no
                    enclosed walls).
                  </div>
                )}

                {/* Lowest Axial Room Modes */}
                {analysis.axial_modes.length > 0 && (
                  <div className="border-t border-[#2b394a] pt-1 space-y-0.5">
                    <div className="text-[9px] uppercase tracking-wider text-[#8cb2d9]">
                      Lowest Axial Room Modes (Estimate · f = n·c / 2L)
                    </div>
                    <div className="grid grid-cols-2 gap-1 font-mono text-[9px] text-[#c2d4e6]">
                      {analysis.axial_modes.slice(0, 8).map((m, idx) => (
                        <div
                          key={`${m.axis_key}-${m.harmonic}-${idx}`}
                          className="flex items-center justify-between bg-[#161c23] border border-[#283545] px-1.5 py-0.5"
                        >
                          <span className="text-[#8ea8c3]">
                            {m.axis_key}·{m.harmonic}
                          </span>
                          <span>{m.freq_hz.toFixed(1)} Hz</span>
                          <span className="text-[#7890a8]">λ {m.wavelength_m.toFixed(2)}m</span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                <div className="text-[9px] text-[#8da4bc] leading-snug border-t border-[#2b394a] pt-1">
                  Estimates only — geometric 6-band image-source paths drive the rendered capture
                  (never replaced by a synthetic RT60 tail).
                </div>
              </div>
            </div>

            <div className="text-[10px] text-[#858585] leading-snug border-t border-[#343434] pt-2">
              Virtual capture space only — does not claim to recover the historical room from the
              original recording.
            </div>
          </>
        )}
      </div>

      {/* Clean 5-Stage Bypassable Post-Dry Pipeline Footer (Stages 3, 4, 5 — All OFF by default) */}
      <div className="border-t border-[#3c3c3c] bg-[#212121] p-2 space-y-2 shrink-0 text-[10px]">
        <div className="space-y-1.5">
          <div className="flex items-center justify-between">
            <span className="uppercase tracking-wider text-[9px] font-semibold text-[#9fb2c5]">
              Optional Post-Dry Stages (Independently Bypassable)
            </span>
            <span className="font-mono text-[9px] text-[#7a8b9c]">
              {!pipelineStages.micEnabled &&
              !pipelineStages.roomEnabled &&
              (!pipelineStages.pluginSlot.enabled || pipelineStages.pluginSlot.pluginId === 'none')
                ? 'All Bypassed (Dry)'
                : 'Active Chain'}
            </span>
          </div>

          {/* Independent Stage Toggles: 3. Mic, 4. Room (10x10x10 padded), 5. Plugin Slot */}
          <div className="grid grid-cols-3 gap-1">
            <button
              type="button"
              onClick={() =>
                onChangePipelineStages?.({
                  ...pipelineStages,
                  micEnabled: !pipelineStages.micEnabled,
                })
              }
              className={`px-1.5 py-1 border text-[9.5px] font-medium transition-colors ${
                pipelineStages.micEnabled
                  ? 'bg-[#21364d] border-[#378ef0] text-[#d6ebff]'
                  : 'bg-[#292929] border-[#3e3e3e] text-[#969696] hover:bg-[#323232]'
              }`}
              title="Stage 3: Optional Microphone Stage (OFF by default)"
            >
              3. Mic: {pipelineStages.micEnabled ? 'ON' : 'OFF'}
            </button>
            <button
              type="button"
              onClick={() =>
                onChangePipelineStages?.({
                  ...pipelineStages,
                  roomEnabled: !pipelineStages.roomEnabled,
                })
              }
              className={`px-1.5 py-1 border text-[9.5px] font-medium transition-colors ${
                pipelineStages.roomEnabled
                  ? 'bg-[#1f362e] border-[#4cc6ac] text-[#d4f7ee]'
                  : 'bg-[#292929] border-[#3e3e3e] text-[#969696] hover:bg-[#323232]'
              }`}
              title="Stage 4: Optional Space / Room Stage (defaults to 10x10x10 ft padded room; OFF by default)"
            >
              4. Room: {pipelineStages.roomEnabled ? 'ON' : 'OFF'}
            </button>
            <button
              type="button"
              onClick={() => {
                const nextEnabled = !pipelineStages.pluginSlot.enabled;
                onChangePipelineStages?.({
                  ...pipelineStages,
                  pluginSlot: {
                    ...pipelineStages.pluginSlot,
                    enabled: nextEnabled,
                    pluginId:
                      nextEnabled && pipelineStages.pluginSlot.pluginId === 'none'
                        ? 'gentle_eq'
                        : pipelineStages.pluginSlot.pluginId,
                  },
                });
              }}
              className={`px-1.5 py-1 border text-[9.5px] font-medium transition-colors ${
                pipelineStages.pluginSlot.enabled && pipelineStages.pluginSlot.pluginId !== 'none'
                  ? 'bg-[#382d1f] border-[#e5a93c] text-[#fae2b4]'
                  : 'bg-[#292929] border-[#3e3e3e] text-[#969696] hover:bg-[#323232]'
              }`}
              title="Stage 5: Optional Plugin / Finishing Slot (OFF / Empty by default)"
            >
              5. Plugin:{' '}
              {pipelineStages.pluginSlot.enabled && pipelineStages.pluginSlot.pluginId !== 'none'
                ? 'ON'
                : 'OFF'}
            </button>
          </div>

          {/* Stage 5 Plugin Slot Controls when enabled */}
          {pipelineStages.pluginSlot.enabled && (
            <div className="bg-[#1b1c1f] border border-[#3a3630] p-1.5 space-y-1 font-mono text-[9.5px]">
              <div className="flex items-center justify-between gap-1.5">
                <span className="text-[#c5b089]">Stage 5 Slot</span>
                <select
                  aria-label="Stage 5 Plugin Slot"
                  value={pipelineStages.pluginSlot.pluginId}
                  onChange={(e) =>
                    onChangePipelineStages?.({
                      ...pipelineStages,
                      pluginSlot: {
                        ...pipelineStages.pluginSlot,
                        pluginId: e.target.value as PluginSlotId,
                      },
                    })
                  }
                  className="px-1.5 py-0.5 bg-[#252525] border border-[#444444] text-[#e4e4e4] text-[10px]"
                >
                  <option value="none">Empty (Bypass)</option>
                  <option value="gentle_eq">Gentle EQ (Low/High Shelf)</option>
                  <option value="soft_dynamics">Soft Dynamics (RMS Comp)</option>
                  <option value="noise_control">Noise Control (Expander)</option>
                  <option value="phase_limiter">Phase Limiter (Lookahead)</option>
                </select>
              </div>
              {pipelineStages.pluginSlot.pluginId === 'gentle_eq' && (
                <div className="grid grid-cols-2 gap-2 text-[9px] text-[#b4b4b4]">
                  <label className="flex items-center gap-1">
                    <span>Low</span>
                    <input
                      type="range"
                      min={-4}
                      max={4}
                      step={0.5}
                      value={pipelineStages.pluginSlot.eqLowDb}
                      onChange={(e) =>
                        onChangePipelineStages?.({
                          ...pipelineStages,
                          pluginSlot: {
                            ...pipelineStages.pluginSlot,
                            eqLowDb: Number(e.target.value),
                          },
                        })
                      }
                      className="flex-1 studio-slider"
                    />
                    <span>{pipelineStages.pluginSlot.eqLowDb.toFixed(1)}dB</span>
                  </label>
                  <label className="flex items-center gap-1">
                    <span>High</span>
                    <input
                      type="range"
                      min={-4}
                      max={4}
                      step={0.5}
                      value={pipelineStages.pluginSlot.eqHighDb}
                      onChange={(e) =>
                        onChangePipelineStages?.({
                          ...pipelineStages,
                          pluginSlot: {
                            ...pipelineStages.pluginSlot,
                            eqHighDb: Number(e.target.value),
                          },
                        })
                      }
                      className="flex-1 studio-slider"
                    />
                    <span>{pipelineStages.pluginSlot.eqHighDb.toFixed(1)}dB</span>
                  </label>
                </div>
              )}
              {pipelineStages.pluginSlot.pluginId !== 'none' &&
                pipelineStages.pluginSlot.pluginId !== 'gentle_eq' && (
                  <div className="flex items-center gap-2 text-[9px] text-[#b4b4b4]">
                    <span>Amount</span>
                    <input
                      type="range"
                      min={0.1}
                      max={1}
                      step={0.05}
                      value={pipelineStages.pluginSlot.amount}
                      onChange={(e) =>
                        onChangePipelineStages?.({
                          ...pipelineStages,
                          pluginSlot: {
                            ...pipelineStages.pluginSlot,
                            amount: Number(e.target.value),
                          },
                        })
                      }
                      className="flex-1 studio-slider"
                    />
                    <span>{Math.round(pipelineStages.pluginSlot.amount * 100)}%</span>
                    {pipelineStages.pluginSlot.pluginId === 'phase_limiter' && (
                      <span className="text-[#e5b96c]">
                        Ceil {pipelineStages.pluginSlot.ceilingDbfs.toFixed(1)}dB
                      </span>
                    )}
                  </div>
                )}
            </div>
          )}
        </div>

        {(pipelineResult || captureResult) && (
          <div className="bg-[#1b1f24] border border-[#333c47] px-2 py-1 font-mono tabular-nums space-y-0.5 text-[9px] text-[#b4c4d4]">
            {pipelineResult ? (
              <>
                <div className="flex justify-between">
                  <span>OUT SHA: {pipelineResult.wav_sha256.slice(0, 12)}</span>
                  <span className={pipelineResult.summary.bit_identical_to_dry ? 'text-[#4cc6ac]' : 'text-[#8bc2ff]'}>
                    {pipelineResult.summary.bit_identical_to_dry
                      ? 'Bit-identical to Dry'
                      : [
                          pipelineResult.summary.mic_stage_active ? 'Mic' : null,
                          pipelineResult.summary.room_stage_active ? 'Room' : null,
                          pipelineResult.summary.plugin_slot_active
                            ? pipelineResult.summary.active_plugin
                            : null,
                        ]
                          .filter(Boolean)
                          .join(' + ')}
                  </span>
                </div>
                <div className="flex justify-between text-[#8da0b3]">
                  <span>Centroid: {Math.round(pipelineResult.spectral.spectral_centroid_hz)} Hz</span>
                  <span>
                    Peak:{' '}
                    {pipelineResult.spectral.peak_dbfs !== null
                      ? `${pipelineResult.spectral.peak_dbfs.toFixed(1)} dB`
                      : '—'}
                  </span>
                </div>
              </>
            ) : captureResult ? (
              <>
                <div className="flex justify-between">
                  <span>RUST SHA: {captureResult.sha256.slice(0, 12)}</span>
                  <span>{captureResult.path_count} paths</span>
                </div>
                <div className="flex justify-between text-[#8da0b3]">
                  <span>
                    1st refl:{' '}
                    {captureResult.first_reflection_ms !== null
                      ? `${captureResult.first_reflection_ms.toFixed(2)} ms`
                      : 'none'}
                  </span>
                  <span>
                    Peak:{' '}
                    {captureResult.peak_dbfs !== null
                      ? `${captureResult.peak_dbfs.toFixed(1)} dB`
                      : '—'}
                  </span>
                </div>
              </>
            ) : null}
          </div>
        )}

        <div className="flex items-center justify-between gap-2">
          <label
            className="flex items-center gap-1.5 text-[11px] text-[#c4c4c4] cursor-pointer"
            title="Auto-render enabled post-dry stages when acoustic rig or toggles change"
          >
            <input
              type="checkbox"
              checked={livePreview}
              disabled={!hasAudio}
              onChange={(e) => onToggleLivePreview(e.target.checked)}
              className="accent-[#378ef0] w-3 h-3"
            />
            <span>Live Preview</span>
          </label>
          <button
            type="button"
            disabled={!hasAudio || pipelineBusy || captureBusy}
            onClick={onRenderPipeline ?? onRerenderCapture}
            className="px-2.5 py-1 text-[11px] font-medium bg-[#378ef0] text-white hover:bg-[#499af5] disabled:opacity-40 disabled:bg-[#3a3a3a] disabled:text-[#8f8f8f]"
          >
            {pipelineBusy || captureBusy ? 'Rendering Rust…' : 'Render Active Pipeline'}
          </button>
        </div>
      </div>
    </aside>
  );
}

function RotationBall({
  aim,
  pulseAxis,
  onDrag,
  onRelease,
}: {
  aim: { headingZDeg: number; tiltXDeg: number };
  pulseAxis: GridDirectionKey | null;
  onDrag: (headingZDeg: number, tiltXDeg: number) => void;
  onRelease: (headingZDeg: number, tiltXDeg: number) => void;
}) {
  const drag = useRef<{ x: number; y: number; h: number; t: number } | null>(null);
  const lastRef = useRef<{ h: number; t: number }>({ h: aim.headingZDeg, t: aim.tiltXDeg });

  const hRad = (aim.headingZDeg * Math.PI) / 180;
  const tRad = (aim.tiltXDeg * Math.PI) / 180;
  const dotX = 28 + Math.sin(hRad) * Math.cos(tRad) * 19;
  const dotY = 28 - Math.sin(tRad) * 19;

  return (
    <div
      role="slider"
      aria-label="Aim Rotation Ball"
      aria-valuemin={-180}
      aria-valuemax={180}
      aria-valuenow={Math.round(aim.headingZDeg)}
      tabIndex={0}
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        drag.current = { x: e.clientX, y: e.clientY, h: aim.headingZDeg, t: aim.tiltXDeg };
        lastRef.current = { h: aim.headingZDeg, t: aim.tiltXDeg };
      }}
      onPointerMove={(e) => {
        if (!drag.current) return;
        let h = drag.current.h + (e.clientX - drag.current.x) * 1.4;
        while (h > 180) h -= 360;
        while (h < -180) h += 360;
        const t = Math.max(-90, Math.min(90, drag.current.t - (e.clientY - drag.current.y) * 1.1));
        lastRef.current = { h, t };
        onDrag(h, t);
      }}
      onPointerUp={() => {
        if (!drag.current) return;
        drag.current = null;
        onRelease(lastRef.current.h, lastRef.current.t);
      }}
      className={`w-14 h-14 shrink-0 border select-none cursor-grab active:cursor-grabbing relative transition-colors ${
        pulseAxis
          ? 'bg-[#1a3326] border-[#52d18a] ring-1 ring-[#52d18a]'
          : 'bg-[#1a1a1a] border-[#3d3d3d]'
      }`}
      title="Drag to rotate mic aim (snaps within 8° of cardinal axes on release)"
    >
      <svg width={56} height={56} viewBox="0 0 56 56">
        <circle cx={28} cy={28} r={22} fill="#232323" stroke="#454545" strokeWidth={1} />
        <ellipse cx={28} cy={28} rx={22} ry={8} fill="none" stroke="#353535" strokeWidth={1} />
        <line x1={28} y1={6} x2={28} y2={50} stroke="#333333" strokeWidth={1} />
        <line x1={28} y1={28} x2={dotX} y2={dotY} stroke="#378ef0" strokeWidth={1.6} />
        <circle cx={dotX} cy={dotY} r={3.5} fill={pulseAxis ? '#52d18a' : '#378ef0'} />
      </svg>
      {pulseAxis && (
        <span className="absolute bottom-0.5 right-0.5 text-[8px] font-mono font-semibold text-[#52d18a]">
          {pulseAxis}
        </span>
      )}
    </div>
  );
}

function MicThumbnailSvg() {
  return (
    <svg width={34} height={48} viewBox="0 0 34 48" className="bg-[#1b1b1b] border border-[#3a3a3a] shrink-0">
      <rect x={11} y={7} width={12} height={18} rx={5} fill="#2f353d" stroke="#8fa6bd" strokeWidth={1.2} />
      <line x1={12} y1={13} x2={22} y2={13} stroke="#637487" strokeWidth={1} />
      <line x1={12} y1={17} x2={22} y2={17} stroke="#637487" strokeWidth={1} />
      <rect x={14} y={25} width={6} height={15} fill="#444444" stroke="#7c7c7c" strokeWidth={1} />
    </svg>
  );
}

function MiniPolarSvg({ pattern }: { pattern: PolarPattern }) {
  const pts: string[] = [];
  const cx = 24;
  const cy = 24;
  const rMax = 18;
  for (let i = 0; i <= 48; i++) {
    const th = (i / 48) * Math.PI * 2;
    const g = Math.abs(polarGain(pattern, Math.cos(th))) * rMax;
    const x = cx + Math.sin(th) * g;
    const y = cy - Math.cos(th) * g;
    pts.push(`${x.toFixed(1)},${y.toFixed(1)}`);
  }
  return (
    <svg width={48} height={48} viewBox="0 0 48 48" className="bg-[#1b1b1b] border border-[#3a3a3a] shrink-0">
      <circle cx={cx} cy={cy} r={18} fill="none" stroke="#313131" strokeWidth={1} />
      <circle cx={cx} cy={cy} r={9} fill="none" stroke="#292929" strokeWidth={1} />
      <line x1={cx} y1={4} x2={cx} y2={44} stroke="#313131" strokeWidth={1} />
      <line x1={4} y1={cy} x2={44} y2={cy} stroke="#313131" strokeWidth={1} />
      <polygon points={pts.join(' ')} fill="rgba(55,142,240,0.22)" stroke="#378ef0" strokeWidth={1.3} />
    </svg>
  );
}
