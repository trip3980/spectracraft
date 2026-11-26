import { useEffect, useMemo, useState } from 'react';
import {
  BookOpen,
  ChevronDown,
  ChevronUp,
  Download,
  Eye,
  EyeOff,
  FileJson,
  Play,
  Plus,
  Redo2,
  Square,
  Trash2,
  Undo2,
} from 'lucide-react';
import {
  DRY_LISTENING_CRITERIA,
  classifyNoteEvidence,
  countPassedDryCriteria,
  isDryListeningPassed,
  summarizeEvidence,
  type DryCriterionKey,
  type DryListeningChecklist,
} from '../lib/contract.ts';
import {
  REGION_FIX_ACTIONS,
  type DryRegion,
  type DryRegionKind,
} from '../lib/dryRegions.ts';
import type { DryRegionEditResponse } from '../lib/studioRig.ts';
import {
  defaultGapRepairStartS,
  formatDbfs,
  shortHash,
  validateRepickInput,
  type GapRepairResult,
  type LayerChannel,
  type NoteEditPatch,
  type NoteSummary,
  type SessionPayload,
} from '../lib/workbench.ts';
import type { ReferenceShape, StructuredSynthesisPrompt } from '../lib/synthesisPrompt.ts';

export type BottomDockTab = 'shape' | 'regions' | 'layers' | 'cues' | 'repair' | 'review';

interface StudioBottomDockProps {
  collapsed: boolean;
  bodyHeightPx?: number;
  embeddedInSidePanel?: boolean;
  onToggleCollapse: () => void;
  activeTab: BottomDockTab;
  onTabChange: (tab: BottomDockTab) => void;
  session: SessionPayload | null;
  selection: [number, number] | null;
  activeDryUrl: string | null;
  activeCuesUrl: string | null;
  // Layers
  layerChannels: LayerChannel[];
  viewedLayerId: LayerChannel['id'] | null;
  includeTexture: boolean;
  onToggleIncludeTexture: (v: boolean) => void;
  onUpdateLayer: (id: LayerChannel['id'], patch: Partial<Pick<LayerChannel, 'gain' | 'muted' | 'solo'>>) => void;
  onToggleViewLayer: (id: LayerChannel['id']) => void;
  onAnalyzeLayers: () => void;
  layersBusy: boolean;
  hasLayerStems: boolean;
  layerMixPlaying?: boolean;
  onToggleLayerMixPlay?: () => void;
  // Cues
  notes: NoteSummary[];
  cuePatches: Record<number, NoteEditPatch>;
  onUpdateCuePatch: (noteIndex: number, patch: NoteEditPatch) => void;
  onRenderEditedCues: () => void;
  cuesBusy: boolean;
  // Local Repair
  onRunGapRepair: (startS: number, gapMs: number) => void;
  repairResult: GapRepairResult | null;
  repairBusy: boolean;
  // Dry Listening Gate (5 contract criteria)
  dryChecklist: DryListeningChecklist;
  onToggleDryCriterion: (key: DryCriterionKey) => void;
  onOpenExportModal: () => void;
  // Reference Shape & Structured Synthesis Prompt
  referenceShape: ReferenceShape | null;
  structuredPrompt: StructuredSynthesisPrompt | null;
  activeDrySha256: string | null;
  capturedSha256: string | null;
  onOpenLineageModal: () => void;
  onApplyCharacterGuess?: (guess: 'natural' | 'analog' | 'digital', straightenRecommended: boolean) => void;
  // Stage 2 Surgical Dry Region Edits
  dryRegions?: DryRegion[];
  selectedRegionId?: string | null;
  canUndoRegion?: boolean;
  canRedoRegion?: boolean;
  onSelectDryRegion?: (id: string | null) => void;
  onQuickAddDryRegion?: (kind: DryRegionKind) => void;
  onUpdateDryRegion?: (id: string, patch: Partial<DryRegion>) => void;
  onDeleteDryRegion?: (id: string) => void;
  onUndoDryRegion?: () => void;
  onRedoDryRegion?: () => void;
  onPreviewDryRegion?: (region: DryRegion) => void;
  onApplyDryRegion?: (region: DryRegion) => void;
  regionEditBusy?: boolean;
  regionEditResult?: DryRegionEditResponse | null;
  regionPreviewPlaying?: boolean;
  regionPreviewAb?: 'after' | 'before';
  onToggleRegionPreviewPlay?: () => void;
  onChangeRegionPreviewAb?: (mode: 'after' | 'before') => void;
  onGenerateReport?: () => void;
  onOpenReport?: () => void;
  hasReport?: boolean;
  reportBusy?: boolean;
}

export function StudioBottomDock({
  collapsed,
  bodyHeightPx = 172,
  embeddedInSidePanel = false,
  onToggleCollapse,
  activeTab,
  onTabChange,
  session,
  selection,
  activeDryUrl,
  activeCuesUrl,
  layerChannels,
  viewedLayerId,
  includeTexture,
  onToggleIncludeTexture,
  onUpdateLayer,
  onToggleViewLayer,
  onAnalyzeLayers,
  layersBusy,
  hasLayerStems,
  layerMixPlaying = false,
  onToggleLayerMixPlay,
  notes,
  cuePatches,
  onUpdateCuePatch,
  onRenderEditedCues,
  cuesBusy,
  onRunGapRepair,
  repairResult,
  repairBusy,
  dryChecklist,
  onToggleDryCriterion,
  onOpenExportModal,
  referenceShape,
  structuredPrompt,
  activeDrySha256,
  capturedSha256,
  onOpenLineageModal,
  onApplyCharacterGuess,
  dryRegions = [],
  selectedRegionId = null,
  canUndoRegion = false,
  canRedoRegion = false,
  onSelectDryRegion,
  onQuickAddDryRegion,
  onUpdateDryRegion,
  onDeleteDryRegion,
  onUndoDryRegion,
  onRedoDryRegion,
  onPreviewDryRegion,
  onApplyDryRegion,
  regionEditBusy = false,
  regionEditResult = null,
  regionPreviewPlaying = false,
  regionPreviewAb = 'after',
  onToggleRegionPreviewPlay,
  onChangeRegionPreviewAb,
  onGenerateReport,
  onOpenReport,
  hasReport = false,
  reportBusy = false,
}: StudioBottomDockProps) {
  const [selectedNoteIdx, setSelectedNoteIdx] = useState<number>(0);
  const [strikeOffset, setStrikeOffset] = useState('0.12');
  const [strikeStrength, setStrikeStrength] = useState('0.75');
  const [strikeErr, setStrikeErr] = useState<string | null>(null);

  const [gapStartS, setGapStartS] = useState('1.600');
  const [gapMs, setGapMs] = useState('2.0');

  useEffect(() => {
    if (!session) return;
    const nextStart = defaultGapRepairStartS(
      session.verification.duration_s,
      selection,
      Number(gapMs) || 2.0,
    );
    setGapStartS(nextStart.toFixed(3));
    // Intentionally sync default start time when session or time selection changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session?.id, session?.verification.duration_s, selection?.[0], selection?.[1]]);

  const passedCount = countPassedDryCriteria(dryChecklist);
  const dryApproved = isDryListeningPassed(dryChecklist);

  const evidenceSummary = useMemo(
    () => summarizeEvidence(notes, session?.transitionEvidence ?? []),
    [notes, session?.transitionEvidence],
  );

  const tabs: { id: BottomDockTab; label: string }[] = embeddedInSidePanel
    ? [
        { id: 'shape', label: 'Shape' },
        { id: 'regions', label: `Regions (${dryRegions.length})` },
        { id: 'layers', label: 'Layers' },
        { id: 'cues', label: `Cues (${notes.length})` },
        { id: 'repair', label: 'Repair' },
        { id: 'review', label: dryApproved ? 'Gate 5/5' : `Gate ${passedCount}/5` },
      ]
    : [
        {
          id: 'shape',
          label: referenceShape
            ? `Reference Shape & Prompt (${referenceShape.pitch_trajectory.note_count}n)`
            : 'Reference Shape & Prompt',
        },
        {
          id: 'regions',
          label: `Region Edits · Dry (${dryRegions.length})`,
        },
        { id: 'layers', label: 'Editable Layers (4)' },
        { id: 'cues', label: `Performance Cues (${notes.length})` },
        { id: 'repair', label: 'Local Repair' },
        {
          id: 'review',
          label: dryApproved
            ? 'Dry Listening Gate · 5/5 Passed'
            : `Dry Listening Gate · ${passedCount}/5`,
        },
      ];

  const selectedRegion = dryRegions.find((r) => r.id === selectedRegionId) ?? dryRegions[0] ?? null;

  const activeNote = notes.find((n) => n.index === selectedNoteIdx) ?? notes[0] ?? null;
  const activePatch: NoteEditPatch | null = activeNote
    ? cuePatches[activeNote.index] ?? {
        index: activeNote.index,
        level_scale: 1,
        re_picks: activeNote.re_picks,
      }
    : null;

  return (
    <div
      className={
        embeddedInSidePanel
          ? 'flex-1 min-h-0 flex flex-col bg-[#1e2025]'
          : 'bg-[#242424] border-t border-[#3c3c3c] flex flex-col shrink-0'
      }
    >
      {/* Dock Tab Bar */}
      <div
        className={
          embeddedInSidePanel
            ? 'bg-[#25272d] border-b border-[#363942] flex flex-wrap items-center gap-0.5 px-1.5 py-1 shrink-0'
            : 'h-7 bg-[#2b2b2b] border-b border-[#383838] flex items-center justify-between px-1.5'
        }
      >
        <div className={embeddedInSidePanel ? 'flex flex-wrap items-center gap-0.5' : 'flex items-center h-full'}>
          {tabs.map((tab) => (
            <button
              key={tab.id}
              type="button"
              onClick={() => {
                onTabChange(tab.id);
                if (!embeddedInSidePanel && collapsed) onToggleCollapse();
              }}
              className={
                embeddedInSidePanel
                  ? `px-2 py-0.5 text-[10px] font-medium border transition-colors ${
                      activeTab === tab.id
                        ? 'bg-[#2f6ec4] border-[#378ef0] text-white'
                        : tab.id === 'review' && !dryApproved && session
                          ? 'bg-[#2a2c33] border-[#3e424d] text-[#e5b96c] hover:text-white'
                          : 'bg-[#2a2c33] border-[#3a3e48] text-[#aeb6c4] hover:text-[#eef2f9] hover:bg-[#333741]'
                    }`
                  : `h-full px-3 text-[11px] font-medium border-r border-[#383838] transition-colors ${
                      !collapsed && activeTab === tab.id
                        ? 'bg-[#242424] text-white border-t-2 border-t-[#378ef0]'
                        : tab.id === 'review' && !dryApproved && session
                          ? 'text-[#e5b96c] hover:text-white hover:bg-[#333333]'
                          : 'text-[#a3a3a3] hover:text-[#e0e0e0] hover:bg-[#333333]'
                    }`
              }
            >
              {tab.label}
            </button>
          ))}
        </div>

        {!embeddedInSidePanel && (
          <button
            type="button"
            onClick={onToggleCollapse}
            className="px-2 py-0.5 text-[#a0a0a0] hover:text-white flex items-center gap-1 text-[11px]"
            title={collapsed ? 'Expand dock' : 'Collapse dock'}
          >
            {collapsed ? <ChevronUp className="w-3.5 h-3.5" /> : <ChevronDown className="w-3.5 h-3.5" />}
          </button>
        )}
      </div>

      {/* Dock Body */}
      {(embeddedInSidePanel || !collapsed) && (
        <div
          style={embeddedInSidePanel ? undefined : { height: `${Math.max(84, bodyHeightPx)}px` }}
          className={
            embeddedInSidePanel
              ? 'flex-1 min-h-0 overflow-y-auto p-2.5 text-[11px]'
              : 'overflow-y-auto p-2.5 text-[11px]'
          }
        >
          {!session ? (
            <div className="h-full flex items-center justify-center text-[#7c7c7c] font-mono text-center px-3">
              Open a single-source mono WAV to extract reference shape, edit cues, and synthesize a fresh dry take.
            </div>
          ) : activeTab === 'shape' ? (
            !referenceShape || !structuredPrompt ? (
              <div className="h-full flex items-center justify-center text-[#8a8a8a] font-mono">
                Extracting Reference Shape in Rust (pitch trajectory, dynamics, 6-band envelope, harmonic/noise balance, uncertainty)…
              </div>
            ) : (
              <div
                className={
                  embeddedInSidePanel
                    ? 'grid grid-cols-1 gap-2.5'
                    : 'grid grid-cols-[1.15fr_1fr] gap-2.5 h-full'
                }
              >
                {/* Left: Extracted Reference Shape (Rust · Read-Only Signal Evidence) */}
                <div className="bg-[#1e1e1e] border border-[#343434] p-2 overflow-y-auto space-y-2">
                  <div className="flex items-center justify-between border-b border-[#2e2e2e] pb-1">
                    <span className="font-medium text-[#e4e4e4]">
                      Extracted Reference Shape (Rust · Read-Only Evidence)
                    </span>
                    <span className="font-mono text-[10px] text-[#4cc6ac]">
                      SHAPE SHA: {shortHash(referenceShape.shape_sha256)}
                    </span>
                  </div>

                  <div className="grid grid-cols-2 gap-2 font-mono text-[10px]">
                    {/* Pitch Trajectory & Bends */}
                    <div className="bg-[#242424] border border-[#323232] p-1.5 space-y-0.5">
                      <div className="text-[#9e9e9e] uppercase tracking-wider text-[9px]">
                        Pitch Trajectory &amp; Bends
                      </div>
                      <div className="text-[#e0e0e0]">
                        {referenceShape.pitch_trajectory.note_count} notes ·{' '}
                        {referenceShape.pitch_trajectory.min_hz !== null &&
                        referenceShape.pitch_trajectory.max_hz !== null
                          ? `${Math.round(referenceShape.pitch_trajectory.min_hz)}–${Math.round(referenceShape.pitch_trajectory.max_hz)} Hz`
                          : 'No tonal pitch'}
                      </div>
                      <div className="text-[#a8a8a8]">
                        Bends: {referenceShape.pitch_trajectory.notes_with_bends} note(s) · max{' '}
                        {referenceShape.pitch_trajectory.max_bend_semitones.toFixed(2)} st
                      </div>
                      <div
                        className={
                          referenceShape.pitch_trajectory.out_of_published_range_notes > 0
                            ? 'text-[#e5b96c]'
                            : 'text-[#888888]'
                        }
                      >
                        {referenceShape.pitch_trajectory.out_of_published_range_notes > 0
                          ? `${referenceShape.pitch_trajectory.out_of_published_range_notes} outside published range (kept as performed)`
                          : 'Within selected source range · never clamped'}
                      </div>
                    </div>

                    {/* Dynamics & Envelope */}
                    <div className="bg-[#242424] border border-[#323232] p-1.5 space-y-0.5">
                      <div className="text-[#9e9e9e] uppercase tracking-wider text-[9px]">
                        Amplitude &amp; Attack / Decay
                      </div>
                      <div className="text-[#e0e0e0]">
                        Peak {referenceShape.dynamics.peak_dbfs.toFixed(1)} dBFS · RMS{' '}
                        {referenceShape.dynamics.rms_dbfs.toFixed(1)} dBFS
                      </div>
                      <div className="text-[#a8a8a8]">
                        Attack ~{referenceShape.attack_and_decay.median_attack_ms.toFixed(0)} ms · Sustain{' '}
                        {Math.round(referenceShape.attack_and_decay.sustain_ratio * 100)}% · Re-picks{' '}
                        {referenceShape.attack_and_decay.total_repicks}
                      </div>
                      <div className="text-[#888888] truncate" title={referenceShape.transitions.transition_note}>
                        Ring overlaps: {referenceShape.transitions.overlapping_ring_notes} · Connected:{' '}
                        {referenceShape.transitions.connected_transitions}
                      </div>
                    </div>

                    {/* 6-Band Spectral Envelope */}
                    <div className="bg-[#242424] border border-[#323232] p-1.5 space-y-1">
                      <div className="flex items-center justify-between">
                        <span className="text-[#9e9e9e] uppercase tracking-wider text-[9px]">
                          Broad Spectral Envelope
                        </span>
                        <span className="text-[#378ef0]">
                          Centroid {Math.round(referenceShape.spectral_envelope.spectral_centroid_hz)} Hz
                        </span>
                      </div>
                      <div className="grid grid-cols-6 gap-1 items-end h-8 pt-1">
                        {referenceShape.spectral_envelope.band_centers_hz.map((hz, i) => {
                          const db = referenceShape.spectral_envelope.band_relative_db[i] ?? -60;
                          const pct = Math.max(8, Math.min(100, ((db + 48) / 48) * 100));
                          return (
                            <div key={hz} className="flex flex-col items-center gap-0.5 h-full justify-end">
                              <div
                                className="w-full bg-[#378ef0]/80"
                                style={{ height: `${pct}%` }}
                                title={`${hz} Hz: ${db.toFixed(1)} dB rel`}
                              />
                              <span className="text-[8px] text-[#888888]">
                                {hz >= 1000 ? `${hz / 1000}k` : hz}
                              </span>
                            </div>
                          );
                        })}
                      </div>
                    </div>

                    {/* Harmonic vs Noise & Bleed Risk */}
                    <div className="bg-[#242424] border border-[#323232] p-1.5 space-y-1">
                      <div className="flex items-center justify-between">
                        <span className="text-[#9e9e9e] uppercase tracking-wider text-[9px]">
                          Harmonic vs Noise · Bleed
                        </span>
                        <span className="text-[#4cc6ac]">
                          {Math.round(referenceShape.harmonic_vs_noise.mean_tonal_fraction * 100)}% tonal
                        </span>
                      </div>
                      <div className="w-full h-2 bg-[#161616] flex overflow-hidden border border-[#343434]">
                        <div
                          className="bg-[#4cc6ac]"
                          style={{
                            width: `${Math.round(referenceShape.harmonic_vs_noise.mean_tonal_fraction * 100)}%`,
                          }}
                        />
                        <div
                          className="bg-[#e5a93c]/70 flex-1"
                        />
                      </div>
                      <div
                        className={
                          referenceShape.harmonic_vs_noise.bleed_risk.startsWith('Elevated')
                            ? 'text-[#e07070]'
                            : referenceShape.harmonic_vs_noise.bleed_risk.startsWith('Moderate')
                              ? 'text-[#e5b96c]'
                              : 'text-[#9e9e9e]'
                        }
                        title={referenceShape.harmonic_vs_noise.bleed_note}
                      >
                        Bleed risk: {referenceShape.harmonic_vs_noise.bleed_risk}
                      </div>
                    </div>
                  </div>

                  {/* Uncertainty Preservation Strip */}
                  <div className="bg-[#242424] border border-[#323232] px-2 py-1 font-mono text-[10px] flex flex-wrap items-center justify-between gap-2">
                    <div className="flex items-center gap-2.5">
                      <span className="text-[#4cc6ac]">
                        Supported: {referenceShape.uncertainty.supported_frames}f
                      </span>
                      <span className="text-[#e5b96c]">
                        Uncertain: {referenceShape.uncertainty.uncertain_frames}f (
                        {referenceShape.uncertainty.uncertain_pct.toFixed(1)}%)
                      </span>
                      <span className="text-[#b084e8]">
                        Out-of-target: {referenceShape.uncertainty.out_of_target_frames}f
                      </span>
                      <span className="text-[#7c7c7c]">
                        Silence: {referenceShape.uncertainty.silence_frames}f
                      </span>
                    </div>
                    <span className="text-[9px] text-[#8a8a8a]">
                      Ambiguous spans preserved — never invented
                    </span>
                  </div>

                  {/* Rough Signal-Character Guess & Loose Profile Suggestions */}
                  {referenceShape.signal_character && (() => {
                    const sc = referenceShape.signal_character;
                    const normalizedGuess = sc.guess.toLowerCase();
                    const canApplyGuess =
                      normalizedGuess === 'natural' ||
                      normalizedGuess === 'analog' ||
                      normalizedGuess === 'digital';
                    const hints = [
                      sc.source_family_guess
                        ? `Family: ${sc.source_family_guess.label} (${sc.source_family_guess.confidence})`
                        : null,
                      sc.mic_style_guess
                        ? `Mic style: ${sc.mic_style_guess.label} (${sc.mic_style_guess.confidence})`
                        : null,
                      sc.room_category_guess
                        ? `Room: ${sc.room_category_guess.label} (${sc.room_category_guess.confidence})`
                        : null,
                    ].filter((v): v is string => Boolean(v));
                    return (
                      <div
                        className="bg-[#22262b] border border-[#36414d] px-2 py-1.5 font-mono text-[10px] space-y-1"
                        title={sc.disclaimer}
                      >
                        <div className="flex items-center justify-between gap-2">
                          <div className="flex items-center gap-2 min-w-0">
                            <span className="text-[#8ea8c3] uppercase text-[9px] shrink-0">
                              Rough Character Guess:
                            </span>
                            <span className="px-1.5 py-0.5 bg-[#1b2938] border border-[#378ef0]/50 text-[#99ccff] font-semibold shrink-0">
                              {sc.guess} ({sc.confidence})
                            </span>
                            {sc.straighten_recommended && (
                              <span className="text-[#e5b96c] text-[9px] truncate">
                                Light straighten suggested ({sc.wandering_frame_pct.toFixed(0)}% wobble)
                              </span>
                            )}
                          </div>
                          {onApplyCharacterGuess && canApplyGuess && (
                            <button
                              type="button"
                              onClick={() =>
                                onApplyCharacterGuess(
                                  normalizedGuess as 'natural' | 'analog' | 'digital',
                                  sc.straighten_recommended,
                                )
                              }
                              className="px-1.5 py-0.5 bg-[#2d4054] hover:bg-[#378ef0] text-[#dceeff] hover:text-white border border-[#4a6a8c] text-[9px] shrink-0"
                            >
                              Use Guess in Dry Stage
                            </button>
                          )}
                        </div>
                        <div className="text-[9px] text-[#9fb2c5] flex flex-wrap gap-x-3 gap-y-0.5">
                          <span>Noise floor: {sc.noise_floor_dbfs.toFixed(0)} dBFS</span>
                          <span>Crest: {sc.crest_factor_db.toFixed(1)} dB</span>
                          <span>Room energy: {Math.round(sc.room_energy_estimate * 100)}%</span>
                          <span>Linearity: {Math.round(sc.linearity_score * 100)}%</span>
                        </div>
                        {hints.length > 0 && (
                          <div className="text-[9px] text-[#8ba0b4] truncate">
                            Loose profile hints: {hints.join(' · ')}
                          </div>
                        )}
                      </div>
                    );
                  })()}
                </div>

                {/* Right: Structured Synthesis Prompt (Live Conditions · Not Free Text) */}
                <div className="bg-[#1e1e1e] border border-[#343434] p-2 overflow-y-auto flex flex-col justify-between gap-1.5">
                  <div className="space-y-1.5">
                    <div className="flex items-center justify-between gap-2 border-b border-[#2e2e2e] pb-1">
                      <span className="font-medium text-[#e4e4e4] truncate">
                        Structured Synthesis Prompt (Live Conditions)
                      </span>
                      <div className="flex items-center gap-1.5 shrink-0">
                        <button
                          type="button"
                          onClick={onOpenLineageModal}
                          className="px-1.5 py-0.5 bg-[#2a2a2a] hover:bg-[#363636] border border-[#444444] text-[#cfcfcf] hover:text-white flex items-center gap-1 text-[10px]"
                          title="View SpectraCraft position relative to existing resynthesis tools & non-goals"
                        >
                          <BookOpen className="w-3 h-3 text-[#378ef0]" />
                          <span>Position &amp; Lineage</span>
                        </button>
                        <span
                          className="font-mono text-[10px] px-1.5 py-0.5 bg-[#25303d] border border-[#378ef0]/50 text-[#8bc2ff]"
                          title="Updates when Source, Mic, Materials, or Room change; invariant under display-only actions"
                        >
                          PROMPT #{structuredPrompt.promptFingerprint}
                        </span>
                      </div>
                    </div>

                    {/* Structured Data Rows */}
                    <div className="grid grid-cols-[74px_1fr] gap-x-2 gap-y-0.5 font-mono text-[10px]">
                      <span className="text-[#808080]">VOICING:</span>
                      <span className="text-[#e0e0e0] truncate">
                        {structuredPrompt.sourceCondition.sourceName} ({structuredPrompt.sourceCondition.family})
                      </span>
                      <span className="text-[#808080]">MIC RIG:</span>
                      <span className="text-[#d6d6d6] truncate">
                        {structuredPrompt.captureCondition.activeMics
                          .map((m) => `${m.label}: ${m.modelName} (${m.patternLabel})`)
                          .join(' + ') || 'None'}
                      </span>
                      <span className="text-[#808080]">ROOM:</span>
                      <span className="text-[#d6d6d6] truncate">
                        {structuredPrompt.captureCondition.roomSummary}
                        {structuredPrompt.captureCondition.directDelayMs !== null
                          ? ` · Direct ${structuredPrompt.captureCondition.directDelayMs.toFixed(3)} ms`
                          : ''}
                      </span>
                      <span className="text-[#808080]">MATERIALS:</span>
                      <span className="text-[#d6d6d6] truncate">
                        {structuredPrompt.captureCondition.materialsSummary}
                      </span>
                    </div>

                    {/* Prompt Statement */}
                    <div className="bg-[#181818] border border-[#2f2f2f] p-1.5 text-[10px] text-[#c7d3df] leading-snug">
                      {structuredPrompt.promptSentence}
                    </div>
                  </div>

                  {/* Provenance Chain & Shared Limits */}
                  <div className="space-y-1 border-t border-[#2e2e2e] pt-1 font-mono text-[9px]">
                    <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[#a0a0a0]">
                      <span>
                        SHAPE: <strong className="text-[#4cc6ac]">{shortHash(referenceShape.shape_sha256)}</strong>
                      </span>
                      <span>
                        DRY: <strong className="text-[#e0e0e0]">{activeDrySha256 ? shortHash(activeDrySha256) : '—'}</strong>
                      </span>
                      <span>
                        CAPTURED:{' '}
                        <strong className="text-[#8bc2ff]">
                          {capturedSha256 ? shortHash(capturedSha256) : 'Pending gate'}
                        </strong>
                      </span>
                      <span className="text-[#4cc6ac]">0% original blended</span>
                    </div>
                    <div className="text-[#7e7e7e] leading-tight">
                      Limits: Shape is signal evidence, not a perfect record of player intent. Changing Source/Mic/Materials/Room updates the prompt &amp; signal; display-only actions leave both unchanged. 100% local Rust DSP.
                    </div>
                  </div>
                </div>
              </div>
            )
          ) : activeTab === 'regions' ? (
            <div
              className={
                embeddedInSidePanel
                  ? 'grid grid-cols-1 gap-2.5'
                  : 'grid grid-cols-[260px_1fr] gap-2.5 h-full'
              }
            >
              {/* Left: Marked Regions List + Add / Undo / Redo */}
              <div className="bg-[#1e1e1e] border border-[#343434] flex flex-col min-h-0">
                <div className="px-2 py-1 bg-[#262626] border-b border-[#343434] flex items-center justify-between gap-1">
                  <span className="font-medium text-[#e0e0e0] text-[10px]">
                    Stage 2 · Dry Regions ({dryRegions.length})
                  </span>
                  <div className="flex items-center gap-1">
                    <button
                      type="button"
                      disabled={!canUndoRegion}
                      onClick={onUndoDryRegion}
                      title="Undo region edit"
                      className="p-0.5 bg-[#2e2e2e] hover:bg-[#3a3a3a] border border-[#424242] disabled:opacity-35"
                    >
                      <Undo2 className="w-3 h-3" />
                    </button>
                    <button
                      type="button"
                      disabled={!canRedoRegion}
                      onClick={onRedoDryRegion}
                      title="Redo region edit"
                      className="p-0.5 bg-[#2e2e2e] hover:bg-[#3a3a3a] border border-[#424242] disabled:opacity-35"
                    >
                      <Redo2 className="w-3 h-3" />
                    </button>
                    <button
                      type="button"
                      onClick={() => onQuickAddDryRegion?.('marquee')}
                      className="px-1.5 py-0.5 bg-[#323232] hover:bg-[#3d3d3d] border border-[#484848] text-[10px] text-[#99ccff]"
                      title="Add rectangular time × freq marquee on Dry Take"
                    >
                      + Marquee
                    </button>
                    <button
                      type="button"
                      onClick={() => onQuickAddDryRegion?.('range')}
                      className="px-1.5 py-0.5 bg-[#323232] hover:bg-[#3d3d3d] border border-[#484848] text-[10px] text-[#7fe0cb]"
                      title="Add in/out time range on Dry Take"
                    >
                      + Range
                    </button>
                  </div>
                </div>
                <div className="flex-1 overflow-y-auto divide-y divide-[#292929]">
                  {dryRegions.length === 0 ? (
                    <div className="p-2.5 text-[10px] text-[#858585] leading-snug">
                      Use the <strong>Marquee</strong> or <strong>Range</strong> tool on the Dry Take waveform/spectrogram (or click + Marquee / + Range) to mark a localized fix. Samples outside stay bit-identical.
                    </div>
                  ) : (
                    dryRegions.map((reg) => {
                      const active = reg.id === selectedRegion?.id;
                      return (
                        <div
                          key={reg.id}
                          onClick={() => onSelectDryRegion?.(reg.id)}
                          className={`px-2 py-1.5 cursor-pointer flex items-center justify-between gap-1.5 ${
                            active ? 'bg-[#378ef0]/25 text-white' : 'text-[#c8c8c8] hover:bg-[#282828]'
                          }`}
                        >
                          <div className="min-w-0">
                            <div className="font-medium text-[10px] truncate">
                              {reg.label}{' '}
                              <span className="font-mono text-[9px] text-[#9db6cc]">
                                ({reg.startS.toFixed(2)}–{reg.endS.toFixed(2)}s
                                {reg.minHz !== null && reg.maxHz !== null
                                  ? ` · ${reg.minHz}–${reg.maxHz}Hz`
                                  : ''}
                                )
                              </span>
                            </div>
                            <div className="text-[9px] text-[#969696] truncate">
                              {reg.action.replace('_', ' ')}
                              {reg.note ? ` · "${reg.note}"` : ''}
                              {reg.appliedCount > 0 ? ` · applied ×${reg.appliedCount}` : ''}
                            </div>
                          </div>
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation();
                              onDeleteDryRegion?.(reg.id);
                            }}
                            className="p-1 text-[#8f8f8f] hover:text-[#e07070]"
                            title="Delete region"
                          >
                            <Trash2 className="w-3 h-3" />
                          </button>
                        </div>
                      );
                    })
                  )}
                </div>
              </div>

              {/* Right: Selected Region Surgical Editor + Solo Loop Preview + A/B */}
              <div className="bg-[#1e1e1e] border border-[#343434] p-2 flex flex-col justify-between gap-1.5 overflow-y-auto">
                {!selectedRegion ? (
                  <div className="h-full flex items-center justify-center font-mono text-[10px] text-[#808080]">
                    Select or draw a region on the Dry Take to attach a short note, solo-preview A/B, and apply a localized crossfaded fix.
                  </div>
                ) : (
                  <>
                    <div className="space-y-1.5">
                      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[#2e2e2e] pb-1">
                        <div className="font-medium text-[#e4e4e4]">
                          {selectedRegion.label}{' '}
                          <span className="font-mono text-[10px] text-[#f0b44c]">
                            {selectedRegion.startS.toFixed(3)}–{selectedRegion.endS.toFixed(3)} s
                            {selectedRegion.kind === 'marquee' &&
                            selectedRegion.minHz !== null &&
                            selectedRegion.maxHz !== null
                              ? ` · ${selectedRegion.minHz}–${selectedRegion.maxHz} Hz`
                              : ' · Full band'}
                          </span>
                        </div>
                        <span className="font-mono text-[9px] text-[#4cc6ac]">
                          Dry stage only · raised-cosine crossfade ({selectedRegion.crossfadeMs} ms)
                        </span>
                      </div>

                      {/* Short Note + Action Presets */}
                      <div className="grid grid-cols-[1fr_auto] gap-2 items-center">
                        <input
                          type="text"
                          value={selectedRegion.note}
                          onChange={(e) =>
                            onUpdateDryRegion?.(selectedRegion.id, { note: e.target.value })
                          }
                          placeholder="Short fix note (e.g. straighten wobble, smooth distortion, reduce hiss)…"
                          className="px-2 py-1 bg-[#252525] border border-[#3f3f3f] text-[#e4e4e4] placeholder-[#787878] text-[11px]"
                        />
                        <div className="flex items-center gap-1">
                          {REGION_FIX_ACTIONS.map((act) => (
                            <button
                              key={act.id}
                              type="button"
                              title={act.shortDesc}
                              onClick={() =>
                                onUpdateDryRegion?.(selectedRegion.id, { action: act.id })
                              }
                              className={`px-1.5 py-1 border text-[10px] ${
                                selectedRegion.action === act.id
                                  ? 'bg-[#378ef0] border-[#378ef0] text-white font-medium'
                                  : 'bg-[#2a2a2a] border-[#404040] text-[#bdbdbd] hover:bg-[#343434]'
                              }`}
                            >
                              {act.label}
                            </button>
                          ))}
                        </div>
                      </div>

                      {/* Strength + Crossfade */}
                      <div className="grid grid-cols-2 gap-3 items-center font-mono text-[10px]">
                        <label className="flex items-center gap-2">
                          <span className="text-[#969696] w-16">Strength</span>
                          <input
                            type="range"
                            min={0.1}
                            max={1}
                            step={0.05}
                            value={selectedRegion.strength}
                            onChange={(e) =>
                              onUpdateDryRegion?.(selectedRegion.id, {
                                strength: Number(e.target.value),
                              })
                            }
                            className="flex-1 studio-slider"
                          />
                          <span className="w-9 text-right text-[#d8d8d8]">
                            {Math.round(selectedRegion.strength * 100)}%
                          </span>
                        </label>
                        <label className="flex items-center gap-2">
                          <span className="text-[#969696] w-16">Crossfade</span>
                          <input
                            type="range"
                            min={4}
                            max={48}
                            step={2}
                            value={selectedRegion.crossfadeMs}
                            onChange={(e) =>
                              onUpdateDryRegion?.(selectedRegion.id, {
                                crossfadeMs: Number(e.target.value),
                              })
                            }
                            className="flex-1 studio-slider"
                          />
                          <span className="w-10 text-right text-[#d8d8d8]">
                            {selectedRegion.crossfadeMs} ms
                          </span>
                        </label>
                      </div>
                    </div>

                    {/* Preview Loop + Before/After A/B + Commit to Dry Take */}
                    <div className="flex flex-wrap items-center justify-between gap-2 pt-1.5 border-t border-[#2e2e2e]">
                      <div className="flex items-center gap-1.5">
                        <button
                          type="button"
                          disabled={regionEditBusy}
                          onClick={() => onPreviewDryRegion?.(selectedRegion)}
                          className="px-2 py-1 bg-[#2f3a47] hover:bg-[#3a4a5c] border border-[#4c6682] text-[#dceeff] text-[10px] font-medium disabled:opacity-40"
                        >
                          {regionEditBusy ? 'Processing Region…' : 'Render Region Preview'}
                        </button>
                        <button
                          type="button"
                          disabled={!regionEditResult}
                          onClick={onToggleRegionPreviewPlay}
                          className={`px-2 py-1 border text-[10px] flex items-center gap-1 disabled:opacity-35 ${
                            regionPreviewPlaying
                              ? 'bg-[#c94b4b] border-[#c94b4b] text-white'
                              : 'bg-[#2a2a2a] border-[#424242] text-[#e0e0e0] hover:bg-[#363636]'
                          }`}
                        >
                          {regionPreviewPlaying ? (
                            <>
                              <Square className="w-3 h-3" /> Stop Loop
                            </>
                          ) : (
                            <>
                              <Play className="w-3 h-3" /> Solo Loop
                            </>
                          )}
                        </button>
                        <div className="flex border border-[#424242] text-[10px] font-mono">
                          <button
                            type="button"
                            disabled={!regionEditResult}
                            onClick={() => onChangeRegionPreviewAb?.('before')}
                            className={`px-2 py-0.5 disabled:opacity-35 ${
                              regionPreviewAb === 'before'
                                ? 'bg-[#5a708a] text-white'
                                : 'bg-[#252525] text-[#9e9e9e]'
                            }`}
                          >
                            Before
                          </button>
                          <button
                            type="button"
                            disabled={!regionEditResult}
                            onClick={() => onChangeRegionPreviewAb?.('after')}
                            className={`px-2 py-0.5 disabled:opacity-35 ${
                              regionPreviewAb === 'after'
                                ? 'bg-[#3d8b6f] text-white'
                                : 'bg-[#252525] text-[#9e9e9e]'
                            }`}
                          >
                            After
                          </button>
                        </div>
                      </div>

                      <div className="flex items-center gap-2">
                        {regionEditResult && (
                          <span className="font-mono text-[9px] text-[#4cc6ac]">
                            {regionEditResult.affected_notes} note(s) · outside bit-identical:{' '}
                            {regionEditResult.outside_bit_identical ? 'YES' : 'NO'}
                          </span>
                        )}
                        <button
                          type="button"
                          disabled={regionEditBusy}
                          onClick={() => onApplyDryRegion?.(selectedRegion)}
                          className="px-2.5 py-1 bg-[#378ef0] hover:bg-[#499af5] disabled:bg-[#383838] text-white font-medium text-[10px]"
                        >
                          Apply Fix to Cached Dry Take
                        </button>
                      </div>
                    </div>
                  </>
                )}
              </div>
            </div>
          ) : activeTab === 'layers' ? (
            <div className="flex flex-col gap-2 h-full">
              <div className="flex flex-wrap items-center justify-between gap-2 pb-1.5 border-b border-[#343434]">
                <div className="flex flex-wrap items-center gap-2">
                  <button
                    type="button"
                    disabled={layersBusy}
                    data-testid="layers-analyze-btn"
                    onClick={onAnalyzeLayers}
                    className="px-2.5 py-1 bg-[#378ef0] hover:bg-[#2c7ad4] disabled:bg-[#3a3a3a] text-white font-medium"
                  >
                    {layersBusy
                      ? 'Analyzing Rust Layers…'
                      : selection
                        ? `Analyze Selection (${selection[0].toFixed(2)}–${selection[1].toFixed(2)} s)`
                        : (() => {
                            const dur = session.verification.duration_s;
                            const s0 = Math.min(20, Math.max(0, dur - 7));
                            const s1 = Math.min(dur, s0 + 7);
                            return `Analyze Excerpt (${s0.toFixed(1)}–${s1.toFixed(1)} s)`;
                          })()}
                  </button>
                  <button
                    type="button"
                    disabled={!hasLayerStems || layersBusy}
                    data-testid="layers-play-mix-btn"
                    onClick={onToggleLayerMixPlay}
                    className={`px-2 py-1 border flex items-center gap-1 font-medium disabled:opacity-35 ${
                      layerMixPlaying
                        ? 'bg-[#c94b4b] border-[#c94b4b] text-white'
                        : 'bg-[#2a2a2a] border-[#424242] text-[#e0e0e0] hover:bg-[#363636]'
                    }`}
                    title="Audition active layer stems using M / S / Gain / Texture controls (inspection only)"
                  >
                    {layerMixPlaying ? (
                      <>
                        <Square className="w-3 h-3" /> Stop Layer Mix
                      </>
                    ) : (
                      <>
                        <Play className="w-3 h-3" /> Play Layer Mix
                      </>
                    )}
                  </button>
                  <label className="flex items-center gap-1.5 text-[#bdbdbd] cursor-pointer">
                    <input
                      type="checkbox"
                      data-testid="layers-include-texture-toggle"
                      checked={includeTexture}
                      onChange={(e) => onToggleIncludeTexture(e.target.checked)}
                      className="accent-[#378ef0] w-3 h-3"
                    />
                    <span>Include modeled texture (experimental)</span>
                  </label>
                </div>
                <span className="font-mono text-[10px] text-[#8b8b8b]">
                  Inspection views never alter output · View changes plot only
                </span>
              </div>

              <div className="grid grid-cols-1 gap-1">
                {layerChannels.map((ch) => {
                  const gainDb = ch.gain <= 0.001 ? -60 : 20 * Math.log10(ch.gain);
                  const isViewed = viewedLayerId === ch.id;
                  return (
                    <div
                      key={ch.id}
                      className="grid grid-cols-[128px_26px_26px_26px_1fr_56px] items-center gap-1.5 px-2 py-1 bg-[#1e1e1e] border border-[#323232]"
                    >
                      <span className="font-medium text-[#dcdcdc] truncate">{ch.label}</span>
                      <button
                        type="button"
                        onClick={() => onUpdateLayer(ch.id, { muted: !ch.muted })}
                        className={`h-5 font-mono text-[10px] border ${
                          ch.muted
                            ? 'bg-[#c94b4b] text-white border-[#c94b4b]'
                            : 'bg-[#2f2f2f] text-[#b5b5b5] border-[#424242] hover:bg-[#3a3a3a]'
                        }`}
                        title="Mute layer"
                      >
                        M
                      </button>
                      <button
                        type="button"
                        onClick={() => onUpdateLayer(ch.id, { solo: !ch.solo })}
                        className={`h-5 font-mono text-[10px] border ${
                          ch.solo
                            ? 'bg-[#d69b2f] text-[#141414] font-semibold border-[#d69b2f]'
                            : 'bg-[#2f2f2f] text-[#b5b5b5] border-[#424242] hover:bg-[#3a3a3a]'
                        }`}
                        title="Solo layer audio"
                      >
                        S
                      </button>
                      <button
                        type="button"
                        disabled={!hasLayerStems}
                        onClick={() => onToggleViewLayer(ch.id)}
                        className={`h-5 flex items-center justify-center border ${
                          isViewed
                            ? 'bg-[#378ef0] text-white border-[#378ef0]'
                            : 'bg-[#2f2f2f] text-[#b5b5b5] border-[#424242] hover:bg-[#3a3a3a] disabled:opacity-40'
                        }`}
                        title="View layer in plot (does not solo audio)"
                      >
                        {isViewed ? <Eye className="w-3 h-3" /> : <EyeOff className="w-3 h-3" />}
                      </button>
                      <input
                        type="range"
                        min={0}
                        max={2}
                        step={0.02}
                        value={ch.gain}
                        onChange={(e) => onUpdateLayer(ch.id, { gain: Number(e.target.value) })}
                        className="w-full studio-slider"
                      />
                      <span className="font-mono text-[10px] tabular-nums text-right text-[#a8a8a8]">
                        {ch.gain <= 0.001 ? '-∞ dB' : `${gainDb >= 0 ? '+' : ''}${gainDb.toFixed(1)} dB`}
                      </span>
                    </div>
                  );
                })}
              </div>
            </div>
          ) : activeTab === 'cues' ? (
            <div className="grid grid-cols-[1fr_300px] gap-3 h-full">
              <div className="flex flex-col min-h-0 border border-[#343434] bg-[#1e1e1e]">
                <div className="px-2 py-1 bg-[#282828] border-b border-[#343434] flex items-center justify-between font-mono text-[10px]">
                  <div className="flex items-center gap-3">
                    <span className="text-[#4cc6ac]">Strong: {evidenceSummary.strongNotes}</span>
                    <span className="text-[#e5a93c]">Weak: {evidenceSummary.weakNotes}</span>
                    <span className="text-[#e07070]">Uncertain: {evidenceSummary.uncertainNotes}</span>
                    <span className="text-[#b084e8]">
                      Out-of-target: {session.verification.cue_summary.out_of_target_frames}f (left out, not noise)
                    </span>
                  </div>
                  {activeCuesUrl && (
                    <a
                      href={`${activeCuesUrl}?download=1`}
                      className="text-[#378ef0] hover:underline flex items-center gap-1"
                    >
                      <FileJson className="w-3 h-3" /> Save Cues
                    </a>
                  )}
                </div>
                <div className="flex-1 overflow-y-auto">
                  <table className="w-full text-left font-mono text-[10px] border-collapse">
                    <thead className="sticky top-0 bg-[#252525] text-[#9b9b9b]">
                      <tr>
                        <th className="px-2 py-1 font-normal">#</th>
                        <th className="px-2 py-1 font-normal">Start</th>
                        <th className="px-2 py-1 font-normal">Dur</th>
                        <th className="px-2 py-1 font-normal">Hz</th>
                        <th className="px-2 py-1 font-normal">Evidence</th>
                        <th className="px-2 py-1 font-normal">Re-picks</th>
                        <th className="px-2 py-1 font-normal">Gain</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-[#292929]">
                      {notes.map((n) => {
                        const patch = cuePatches[n.index];
                        const active = selectedNoteIdx === n.index;
                        const ev = classifyNoteEvidence(n);
                        return (
                          <tr
                            key={n.index}
                            onClick={() => setSelectedNoteIdx(n.index)}
                            className={`cursor-pointer ${
                              active ? 'bg-[#378ef0]/30 text-white' : 'text-[#c8c8c8] hover:bg-[#292929]'
                            }`}
                          >
                            <td className="px-2 py-1 tabular-nums">{n.index + 1}</td>
                            <td className="px-2 py-1 tabular-nums">{n.start.toFixed(3)}s</td>
                            <td className="px-2 py-1 tabular-nums">{n.duration_s.toFixed(3)}s</td>
                            <td className="px-2 py-1 tabular-nums">{n.median_hz.toFixed(1)}</td>
                            <td
                              className={`px-2 py-1 ${
                                ev === 'strong_coherent'
                                  ? 'text-[#4cc6ac]'
                                  : ev === 'weak_consistent'
                                    ? 'text-[#e5a93c]'
                                    : 'text-[#e07070]'
                              }`}
                            >
                              {ev === 'strong_coherent'
                                ? 'Strong'
                                : ev === 'weak_consistent'
                                  ? 'Weak'
                                  : 'Uncertain'}
                            </td>
                            <td className="px-2 py-1 tabular-nums">
                              {(patch?.re_picks ?? n.re_picks).length}
                            </td>
                            <td className="px-2 py-1 tabular-nums">
                              {(patch?.level_scale ?? 1).toFixed(2)}×
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              </div>

              <div className="flex flex-col justify-between bg-[#1e1e1e] border border-[#343434] p-2">
                {activeNote && activePatch ? (
                  <>
                    <div className="space-y-1.5">
                      <div className="flex items-center justify-between text-[#dcdcdc] font-medium">
                        <span>
                          Note #{activeNote.index + 1} ({activeNote.start.toFixed(2)}–{activeNote.end.toFixed(2)}s)
                        </span>
                        <span className="font-mono text-[10px] text-[#4cc6ac]">
                          {activeNote.median_hz.toFixed(1)} Hz
                        </span>
                      </div>
                      <div className="flex items-center gap-2">
                        <span className="text-[#9e9e9e] w-12">Gain ×</span>
                        <input
                          type="range"
                          min={0}
                          max={2}
                          step={0.05}
                          value={activePatch.level_scale}
                          onChange={(e) =>
                            onUpdateCuePatch(activeNote.index, {
                              ...activePatch,
                              level_scale: Number(e.target.value),
                            })
                          }
                          className="flex-1 studio-slider"
                        />
                        <span className="font-mono tabular-nums w-9 text-right">
                          {activePatch.level_scale.toFixed(2)}
                        </span>
                      </div>
                      <div className="flex items-center gap-1">
                        <input
                          type="text"
                          value={strikeOffset}
                          onChange={(e) => setStrikeOffset(e.target.value)}
                          placeholder="off s"
                          className="w-16 px-1.5 py-0.5 bg-[#262626] border border-[#3d3d3d] font-mono text-[10px]"
                        />
                        <input
                          type="text"
                          value={strikeStrength}
                          onChange={(e) => setStrikeStrength(e.target.value)}
                          placeholder="str"
                          className="w-14 px-1.5 py-0.5 bg-[#262626] border border-[#3d3d3d] font-mono text-[10px]"
                        />
                        <button
                          type="button"
                          onClick={() => {
                            const off = Number(strikeOffset);
                            const str = Number(strikeStrength);
                            const err = validateRepickInput(activeNote.duration_s, off, str);
                            if (err) {
                              setStrikeErr(err);
                              return;
                            }
                            setStrikeErr(null);
                            onUpdateCuePatch(activeNote.index, {
                              ...activePatch,
                              re_picks: [...activePatch.re_picks, { offset_s: off, strength: str }],
                            });
                          }}
                          className="px-2 py-0.5 bg-[#323232] hover:bg-[#3d3d3d] border border-[#454545] flex items-center gap-1"
                        >
                          <Plus className="w-3 h-3" /> Re-pick
                        </button>
                      </div>
                      {strikeErr && <div className="text-[10px] text-[#e07070]">{strikeErr}</div>}
                    </div>
                    <div className="space-y-1">
                      <button
                        type="button"
                        disabled={cuesBusy}
                        onClick={onRenderEditedCues}
                        className="w-full py-1 bg-[#378ef0] hover:bg-[#2c7ad4] disabled:bg-[#383838] text-white font-medium"
                      >
                        {cuesBusy ? 'Synthesizing Dry (Source-Free)…' : 'Synthesize New Dry WAV From Cues'}
                      </button>
                      <div className="font-mono text-[9px] text-[#848484] text-center">
                        Cues are sole source of truth · Resets dry listening gate to 0/5
                      </div>
                    </div>
                  </>
                ) : (
                  <div className="text-[#8a8a8a]">Select a note cue to edit.</div>
                )}
              </div>
            </div>
          ) : activeTab === 'repair' ? (
            <div className="flex items-start gap-4">
              <div className="w-64 space-y-2 bg-[#1e1e1e] border border-[#343434] p-2.5">
                <div className="text-[10px] uppercase tracking-wider text-[#909090]">
                  Explicit Local Interval (0.25–4.0 ms)
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <label className="flex flex-col gap-0.5">
                    <span className="text-[10px] text-[#8d8d8d]">Start (s)</span>
                    <input
                      type="number"
                      step={0.01}
                      value={gapStartS}
                      onChange={(e) => setGapStartS(e.target.value)}
                      className="px-1.5 py-1 bg-[#262626] border border-[#3e3e3e] font-mono tabular-nums"
                    />
                  </label>
                  <label className="flex flex-col gap-0.5">
                    <span className="text-[10px] text-[#8d8d8d]">Gap (ms)</span>
                    <input
                      type="number"
                      step={0.25}
                      min={0.25}
                      max={4}
                      value={gapMs}
                      onChange={(e) => setGapMs(e.target.value)}
                      className="px-1.5 py-1 bg-[#262626] border border-[#3e3e3e] font-mono tabular-nums"
                    />
                  </label>
                </div>
                <button
                  type="button"
                  disabled={repairBusy}
                  onClick={() => onRunGapRepair(Number(gapStartS), Number(gapMs))}
                  className="w-full py-1 bg-[#378ef0] hover:bg-[#2c7ad4] disabled:bg-[#3a3a3a] text-white font-medium"
                >
                  {repairBusy ? 'Running Rust repair-gap…' : 'Run Local Gap Repair'}
                </button>
              </div>

              <div className="flex-1 bg-[#1e1e1e] border border-[#343434] p-2.5 space-y-1.5 font-mono text-[11px]">
                <div className="text-[#9e9e9e]">
                  Separate Rust tool (`repair-gap`): replaces only the explicit damaged span and preserves all unaffected samples unchanged.
                </div>
                {repairResult && (
                  <div className="pt-1 border-t border-[#323232] space-y-1">
                    <div className={repairResult.status === 'filled' ? 'text-[#4cc6ac]' : 'text-[#e5a93c]'}>
                      Status: {repairResult.status} ({repairResult.repairedFrames} frames)
                    </div>
                    <div className="text-[#b8b8b8]">{repairResult.message}</div>
                    {repairResult.repairedWavUrl && (
                      <a
                        href={`${repairResult.repairedWavUrl}?download=1`}
                        className="inline-block text-[#378ef0] hover:underline"
                      >
                        Download repaired WAV
                      </a>
                    )}
                  </div>
                )}
              </div>
            </div>
          ) : (
            <div className="grid grid-cols-[1.25fr_1fr] gap-3 h-full">
              {/* 5-Criterion Human Listening Gate */}
              <div className="bg-[#1e1e1e] border border-[#343434] p-2.5 flex flex-col justify-between">
                <div className="flex items-center justify-between">
                  <span className="font-medium text-[#e4e4e4]">
                    Human Listening Review ({passedCount}/5)
                  </span>
                  <span
                    className={`font-mono text-[10px] ${
                      dryApproved ? 'text-[#4cc6ac]' : 'text-[#e5b96c]'
                    }`}
                  >
                    {dryApproved ? 'Reviewed · Accepted' : 'Candidate · Unreviewed'}
                  </span>
                </div>

                <div className="grid grid-cols-5 gap-1.5 my-1.5">
                  {DRY_LISTENING_CRITERIA.map((item) => {
                    const checked = dryChecklist[item.key];
                    return (
                      <label
                        key={item.key}
                        title={item.detail}
                        className={`px-2 py-1.5 border flex flex-col gap-1 cursor-pointer transition-colors ${
                          checked
                            ? 'bg-[#1f2e28] border-[#3d8b6f] text-[#d9f5ea]'
                            : 'bg-[#262626] border-[#3b3b3b] text-[#bdbdbd] hover:bg-[#2f2f2f]'
                        }`}
                      >
                        <div className="flex items-center gap-1.5">
                          <input
                            type="checkbox"
                            checked={checked}
                            onChange={() => onToggleDryCriterion(item.key)}
                            className="accent-[#4cc6ac] w-3 h-3"
                          />
                          <span className="font-medium text-[11px]">{item.label}</span>
                        </div>
                        <span className="text-[9px] text-[#8c8c8c] leading-tight line-clamp-2">
                          {item.detail}
                        </span>
                      </label>
                    );
                  })}
                </div>

                <div className="font-mono text-[10px] text-[#888888]">
                  Visuals and metrics are supporting evidence only — generated audio stays Candidate until human A/B listening review is recorded for the current dry revision.
                </div>
              </div>

              {/* Clean Dry Deliverable & Audit Readout */}
              <div className="bg-[#1e1e1e] border border-[#343434] p-2.5 flex flex-col justify-between font-mono text-[10px]">
                <div className="grid grid-cols-2 gap-x-3 gap-y-1">
                  <div>
                    <span className="text-[#7f7f7f]">ORIGINAL: </span>
                    <span className="text-[#d6d6d6]">
                      {shortHash(session.verification.source_sha256)}
                    </span>
                  </div>
                  <div>
                    <span className="text-[#7f7f7f]">DRY PEAK: </span>
                    <span className="text-[#4cc6ac]">
                      {formatDbfs(session.verification.dry_peak_dbfs)} (unmastered)
                    </span>
                  </div>
                  <div>
                    <span className="text-[#7f7f7f]">A/B MATCH: </span>
                    <span className="text-[#d6d6d6]">
                      {session.verification.ab_dry_match_gain_db >= 0 ? '+' : ''}
                      {session.verification.ab_dry_match_gain_db.toFixed(2)} dB
                    </span>
                  </div>
                  <div>
                    <span className="text-[#7f7f7f]">CUE RERUN: </span>
                    <span className="text-[#4cc6ac]">
                      {session.verification.cue_only_rerun_identical ? 'Bit-identical' : 'Pending'}
                    </span>
                  </div>
                </div>

                <div className="flex items-center gap-1.5 pt-2 border-t border-[#2e2e2e]">
                  {activeDryUrl && (
                    <a
                      href={`${activeDryUrl}?download=1`}
                      className="px-2 py-1 bg-[#323232] hover:bg-[#3d3d3d] border border-[#444444] text-[#e0e0e0] flex items-center gap-1"
                    >
                      <Download className="w-3 h-3" /> Dry WAV
                    </a>
                  )}
                  {activeCuesUrl && (
                    <a
                      href={`${activeCuesUrl}?download=1`}
                      className="px-2 py-1 bg-[#323232] hover:bg-[#3d3d3d] border border-[#444444] text-[#e0e0e0] flex items-center gap-1"
                    >
                      <FileJson className="w-3 h-3" /> Cues JSON
                    </a>
                  )}
                  {onGenerateReport && (
                    <button
                      type="button"
                      disabled={reportBusy}
                      data-testid="dock-generate-report-btn"
                      onClick={onGenerateReport}
                      className="px-2 py-1 bg-[#1f2c3d] hover:bg-[#273a52] border border-[#355378] text-[#cfe4ff] disabled:opacity-40"
                    >
                      {reportBusy ? 'Auditing…' : 'Generate Report'}
                    </button>
                  )}
                  {hasReport && onOpenReport && (
                    <button
                      type="button"
                      data-testid="dock-open-report-btn"
                      onClick={onOpenReport}
                      className="px-2 py-1 bg-[#1b3048] hover:bg-[#244061] border border-[#3b6ca3] text-[#dcebff]"
                    >
                      View Report
                    </button>
                  )}
                  <button
                    type="button"
                    disabled={!dryApproved}
                    onClick={onOpenExportModal}
                    className="ml-auto px-2.5 py-1 bg-[#378ef0] hover:bg-[#2c7ad4] disabled:bg-[#343434] disabled:text-[#6f6f6f] text-white font-sans font-medium"
                  >
                    {dryApproved ? 'Export Variants…' : 'Variants Locked'}
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
