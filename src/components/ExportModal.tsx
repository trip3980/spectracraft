import { useState } from 'react';
import { Download, FileJson, Lock, Share2, X } from 'lucide-react';
import {
  ALL_VARIANTS,
  VARIANT_DESCRIPTORS,
  formatDbfs,
  toggleVariantSelection,
  type SessionPayload,
  type VariantKey,
  type VariantsManifest,
} from '../lib/workbench.ts';
import type { PipelineRenderResponse } from '../lib/studioRig.ts';

type DawTargetId = 'audition' | 'logic' | 'daw' | 'soundcraft';

interface DawInterchangeFile {
  role: string;
  relative_path: string;
  sha256: string;
  bytes: number;
  downloadUrl: string | null;
}

interface DawInterchangeResult {
  target_id: string;
  target_name: string;
  sample_rate: number;
  bit_depth: string;
  stem_count: number;
  marker_count: number;
  files: DawInterchangeFile[];
}

const DAW_TARGETS: Array<{ id: DawTargetId; label: string; detail: string }> = [
  { id: 'audition', label: 'Adobe Audition', detail: 'WAV stems + .sesx + Markers TSV' },
  { id: 'logic', label: 'Logic Pro', detail: 'WAV stems + Marker CSV + AU JSON' },
  { id: 'daw', label: 'Standard DAW', detail: 'WAV stems + CSV + Preset JSON' },
  { id: 'soundcraft', label: 'ArtCraft SoundCraft', detail: 'Audio Files/ + .scraft.json' },
];

interface ExportModalProps {
  open: boolean;
  onClose: () => void;
  session: SessionPayload | null;
  dryReviewApproved: boolean;
  dryPassedCount: number;
  onOpenDryReview: () => void;
  selectedVariants: VariantKey[];
  onChangeSelectedVariants: (next: VariantKey[]) => void;
  onRenderVariants: () => void;
  variantsBusy: boolean;
  variants: VariantsManifest | null;
  activeDryUrl: string | null;
  activeCuesUrl: string | null;
  activeCuesRunId?: string | null;
  pipelineResult?: PipelineRenderResponse | null;
}

export function ExportModal({
  open,
  onClose,
  session,
  dryReviewApproved,
  dryPassedCount,
  onOpenDryReview,
  selectedVariants,
  onChangeSelectedVariants,
  onRenderVariants,
  variantsBusy,
  variants,
  activeDryUrl,
  activeCuesUrl,
  activeCuesRunId = null,
  pipelineResult = null,
}: ExportModalProps) {
  const [dawTarget, setDawTarget] = useState<DawTargetId>('audition');
  const [dawBitDepth, setDawBitDepth] = useState<'24' | '32'>('24');
  const [dawBusy, setDawBusy] = useState(false);
  const [dawError, setDawError] = useState<string | null>(null);
  const [dawResult, setDawResult] = useState<DawInterchangeResult | null>(null);

  if (!open || !session) return null;

  async function handleDawExport() {
    if (!session) return;
    setDawBusy(true);
    setDawError(null);
    try {
      const resp = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/daw-export`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          target: dawTarget,
          bitDepth: dawBitDepth,
          cuesRunId: activeCuesRunId ?? '',
          pipelineRunId: pipelineResult?.runId ?? '',
        }),
      });
      const data = await resp.json();
      if (!resp.ok) {
        throw new Error(data.error || 'DAW interchange export failed.');
      }
      setDawResult(data as DawInterchangeResult);
    } catch (err) {
      setDawError(err instanceof Error ? err.message : String(err));
    } finally {
      setDawBusy(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 bg-black/60 flex items-center justify-center p-4">
      <div className="w-[580px] max-w-full max-h-[90vh] overflow-y-auto bg-[#262626] border border-[#444444] text-[#dcdcdc] flex flex-col">
        <div className="h-8 px-3 bg-[#2e2e2e] border-b border-[#3e3e3e] flex items-center justify-between shrink-0">
          <span className="font-medium text-[12px]">Export — {session.label}</span>
          <button type="button" onClick={onClose} className="text-[#9a9a9a] hover:text-white">
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="p-3 space-y-3 text-[12px]">
          {/* Stage 2–3 Deliverables: Unmastered Dry WAV & Editable Cues JSON */}
          <div className="bg-[#1e1e1e] border border-[#363636] p-2.5 space-y-2">
            <div className="flex items-center justify-between gap-2">
              <div>
                <div className="font-medium text-[#e4e4e4]">Dry Take WAV (Unmastered)</div>
                <div className="font-mono text-[10px] text-[#909090]">
                  Peak {formatDbfs(session.verification.dry_peak_dbfs)} · headroom preserved · original never overwritten
                </div>
              </div>
              {activeDryUrl && (
                <a
                  href={`${activeDryUrl}?download=1`}
                  className="px-2.5 py-1 bg-[#323232] hover:bg-[#3d3d3d] border border-[#464646] text-[#e0e0e0] flex items-center gap-1.5 text-[11px] shrink-0"
                >
                  <Download className="w-3.5 h-3.5" /> Save Dry WAV
                </a>
              )}
            </div>

            <div className="pt-2 border-t border-[#2e2e2e] flex items-center justify-between gap-2">
              <div>
                <div className="font-medium text-[#e4e4e4]">Editable Performance Cues (.json)</div>
                <div className="font-mono text-[10px] text-[#909090]">
                  Sole source of truth · regenerate dry take without re-running analysis
                </div>
              </div>
              {activeCuesUrl && (
                <a
                  href={`${activeCuesUrl}?download=1`}
                  className="px-2.5 py-1 bg-[#323232] hover:bg-[#3d3d3d] border border-[#464646] text-[#e0e0e0] flex items-center gap-1.5 text-[11px] shrink-0"
                >
                  <FileJson className="w-3.5 h-3.5" /> Save Cues JSON
                </a>
              )}
            </div>

            {pipelineResult?.wavUrl && (
              <div className="pt-2 border-t border-[#2e2e2e] flex items-center justify-between gap-2">
                <div className="min-w-0">
                  <div className="font-medium text-[#e4e4e4]">
                    Final Pipeline Take WAV (Stages 3–5)
                  </div>
                  <div className="font-mono text-[10px] text-[#909090] truncate">
                    {pipelineResult.summary.bit_identical_to_dry
                      ? 'Post-dry stages bypassed (bit-identical to dry)'
                      : [
                          pipelineResult.summary.mic_stage_active ? 'Mic' : null,
                          pipelineResult.summary.room_stage_active ? 'Room' : null,
                          pipelineResult.summary.plugin_slot_active
                            ? pipelineResult.summary.active_plugin
                            : null,
                        ]
                          .filter(Boolean)
                          .join(' + ') || 'Active pipeline'}{' '}
                    · Peak{' '}
                    {pipelineResult.spectral.peak_dbfs !== null
                      ? formatDbfs(pipelineResult.spectral.peak_dbfs)
                      : '—'}{' '}
                    · SHA {pipelineResult.wav_sha256.slice(0, 10)}
                  </div>
                </div>
                <a
                  href={`${pipelineResult.wavUrl}?download=1`}
                  data-testid="export-pipeline-wav-link"
                  className="px-2.5 py-1 bg-[#1e3552] hover:bg-[#27466b] border border-[#3a689e] text-[#dcebff] flex items-center gap-1.5 text-[11px] shrink-0"
                >
                  <Download className="w-3.5 h-3.5" /> Save Pipeline WAV
                </a>
              </div>
            )}
          </div>

          {/* DAW & External Audio-Editor Interchange */}
          <div className="bg-[#1e1e1e] border border-[#363636] p-2.5 space-y-2">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-1.5 font-medium text-[#e4e4e4]">
                <Share2 className="w-3.5 h-3.5 text-[#378ef0]" />
                <span>DAW &amp; Audio-Editor Interchange</span>
              </div>
              <div className="flex items-center gap-1 text-[10px] font-mono">
                <button
                  type="button"
                  onClick={() => setDawBitDepth('24')}
                  className={`px-1.5 py-0.5 border ${
                    dawBitDepth === '24'
                      ? 'bg-[#2a3f59] border-[#4b7bb5] text-[#e5f0ff]'
                      : 'bg-[#252525] border-[#3a3a3a] text-[#9b9b9b]'
                  }`}
                >
                  24-bit TPDF
                </button>
                <button
                  type="button"
                  onClick={() => setDawBitDepth('32')}
                  className={`px-1.5 py-0.5 border ${
                    dawBitDepth === '32'
                      ? 'bg-[#2a3f59] border-[#4b7bb5] text-[#e5f0ff]'
                      : 'bg-[#252525] border-[#3a3a3a] text-[#9b9b9b]'
                  }`}
                >
                  32-bit Float
                </button>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-1.5">
              {DAW_TARGETS.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  data-testid={`daw-target-${t.id}`}
                  onClick={() => setDawTarget(t.id)}
                  className={`text-left px-2 py-1.5 border ${
                    dawTarget === t.id
                      ? 'bg-[#1f3046] border-[#378ef0] text-[#ebf4ff]'
                      : 'bg-[#252525] border-[#353535] text-[#cccccc] hover:bg-[#2c2c2c]'
                  }`}
                >
                  <div className="font-medium text-[11px]">{t.label}</div>
                  <div className="font-mono text-[9.5px] text-[#909090]">{t.detail}</div>
                </button>
              ))}
            </div>

            <div className="flex items-center justify-between gap-2 pt-0.5">
              <span className="font-mono text-[10px] text-[#8d8d8d]">
                Standard WAV stems + markers + EngineStateSnapshot JSON · zero original PCM
              </span>
              <button
                type="button"
                data-testid="daw-export-btn"
                disabled={dawBusy}
                onClick={handleDawExport}
                className="px-2.5 py-1 bg-[#2a5c99] hover:bg-[#326db5] disabled:bg-[#383838] disabled:text-[#707070] text-white font-medium text-[11px] shrink-0"
              >
                {dawBusy ? 'Exporting Bundle…' : 'Export DAW Bundle'}
              </button>
            </div>

            {dawError && (
              <div className="px-2 py-1 bg-[#3a1f1f] border border-[#733434] text-[#f3a8a8] text-[11px]">
                {dawError}
              </div>
            )}

            {dawResult && (
              <div className="border-t border-[#343434] pt-2 space-y-1">
                <div className="font-mono text-[10px] text-[#8bc4ff]">
                  {dawResult.target_name} · {dawResult.sample_rate} Hz · {dawResult.bit_depth} ·{' '}
                  {dawResult.stem_count} stems · {dawResult.marker_count} markers
                </div>
                {dawResult.files.map((f) => (
                  <div
                    key={f.relative_path}
                    className="flex items-center justify-between text-[10.5px] font-mono bg-[#232323] px-2 py-1 border border-[#323232]"
                  >
                    <span className="text-[#d6d6d6] truncate">{f.relative_path}</span>
                    {f.downloadUrl && (
                      <a
                        href={f.downloadUrl}
                        className="text-[#378ef0] hover:underline flex items-center gap-1 shrink-0 ml-2"
                      >
                        <Download className="w-3 h-3" /> Save
                      </a>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* Stage 4 Post-Processing Variants (Strictly Gated by 5/5 Human Dry Listening Check) */}
          <div className="bg-[#1e1e1e] border border-[#363636] p-2.5 space-y-2">
            <div className="flex items-center justify-between">
              <span className="font-medium text-[#e4e4e4]">
                Stage 4 · Post-Processing Variants (Individual or All)
              </span>
              <button
                type="button"
                onClick={() =>
                  onChangeSelectedVariants(
                    selectedVariants.length === ALL_VARIANTS.length ? ['natural'] : [...ALL_VARIANTS],
                  )
                }
                className="text-[11px] text-[#378ef0] hover:underline"
              >
                {selectedVariants.length === ALL_VARIANTS.length ? 'Select single' : 'Select all'}
              </button>
            </div>

            {!dryReviewApproved && (
              <div className="px-2.5 py-2 bg-[#2a2216] border border-[#6e5226] text-[#e5b96c] text-[11px] flex items-center justify-between gap-2">
                <span className="flex items-center gap-1.5">
                  <Lock className="w-3.5 h-3.5 shrink-0" />
                  <span>
                    Locked until dry listening check passes ({dryPassedCount}/5: rhythm, phrasing, dynamics, sustain, no artifacts).
                  </span>
                </span>
                <button
                  type="button"
                  onClick={() => {
                    onClose();
                    onOpenDryReview();
                  }}
                  className="px-2 py-0.5 bg-[#3a2e1c] hover:bg-[#4b3b23] border border-[#7c5d2c] text-[#f4d08b] shrink-0"
                >
                  Open Check
                </button>
              </div>
            )}

            <div className="space-y-1">
              {VARIANT_DESCRIPTORS.map((v) => {
                const checked = selectedVariants.includes(v.key);
                return (
                  <label
                    key={v.key}
                    className="flex items-center justify-between px-2 py-1 bg-[#252525] border border-[#343434] cursor-pointer hover:bg-[#2c2c2c]"
                  >
                    <div className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={checked}
                        onChange={() =>
                          onChangeSelectedVariants(toggleVariantSelection(selectedVariants, v.key))
                        }
                        className="accent-[#378ef0]"
                      />
                      <span className="font-medium text-[#e0e0e0]">{v.title}</span>
                      <span className="text-[11px] text-[#8d8d8d]">— {v.subtitle}</span>
                    </div>
                  </label>
                );
              })}
            </div>

            <div className="flex items-center justify-end gap-2 pt-1">
              <button
                type="button"
                disabled={!dryReviewApproved || selectedVariants.length === 0 || variantsBusy}
                onClick={onRenderVariants}
                className="px-3 py-1 bg-[#378ef0] hover:bg-[#2c7ad4] disabled:bg-[#383838] disabled:text-[#707070] text-white font-medium text-[11px]"
              >
                {variantsBusy ? 'Rendering Rust Variants…' : `Render Selected (${selectedVariants.length})`}
              </button>
            </div>

            {variants && variants.exports.length > 0 && (
              <div className="border-t border-[#343434] pt-2 space-y-1">
                {variants.exports.map((ex) => (
                  <div
                    key={ex.file}
                    className="flex items-center justify-between text-[11px] font-mono bg-[#232323] px-2 py-1 border border-[#323232]"
                  >
                    <span className="text-[#d6d6d6]">{ex.label}</span>
                    <span className="text-[#8e8e8e]">{formatDbfs(ex.peak_dbfs)}</span>
                    {ex.url && (
                      <a
                        href={`${ex.url}?download=1`}
                        className="text-[#378ef0] hover:underline flex items-center gap-1"
                      >
                        <Download className="w-3 h-3" /> WAV
                      </a>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
