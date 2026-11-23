import React from 'react';
import { Download, FileJson, Lock, X } from 'lucide-react';
import {
  ALL_VARIANTS,
  VARIANT_DESCRIPTORS,
  formatDbfs,
  toggleVariantSelection,
  type SessionPayload,
  type VariantKey,
  type VariantsManifest,
} from '../lib/workbench.ts';

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
}: ExportModalProps) {
  if (!open || !session) return null;

  return (
    <div className="fixed inset-0 z-50 bg-black/60 flex items-center justify-center p-4">
      <div className="w-[540px] max-w-full bg-[#262626] border border-[#444444] text-[#dcdcdc] flex flex-col">
        <div className="h-8 px-3 bg-[#2e2e2e] border-b border-[#3e3e3e] flex items-center justify-between">
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
