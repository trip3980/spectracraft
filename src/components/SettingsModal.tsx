import { useEffect, useState } from 'react';
import {
  CheckCircle2,
  Download,
  FolderCog,
  RefreshCw,
  RotateCcw,
  Settings,
  ShieldCheck,
  X,
} from 'lucide-react';
import {
  DEFAULT_SPECTRACRAFT_SETTINGS,
  type SettingsSnapshot,
  type SpectraCraftSettings,
  type UpdateActionKind,
  type UpdateActionResult,
  type UpdateCheckReport,
} from '../lib/settingsAndUpdates.ts';

export type SettingsFocusSection = 'appearance' | 'workspace' | 'updates' | 'paths' | null;

interface SettingsModalProps {
  open: boolean;
  focusSection?: SettingsFocusSection;
  onClose: () => void;
  snapshot: SettingsSnapshot | null;
  saving: boolean;
  error: string | null;
  onSave: (next: SpectraCraftSettings) => Promise<boolean>;
  onResetDefaults: () => Promise<boolean>;
  updateReport: UpdateCheckReport | null;
  updateCheckBusy: boolean;
  updateActionBusy: UpdateActionKind | null;
  updateActionResult: UpdateActionResult | null;
  onCheckUpdates: (online: boolean) => Promise<void>;
  onUpdateAction: (action: UpdateActionKind) => Promise<void>;
}

export function SettingsModal({
  open,
  focusSection = null,
  onClose,
  snapshot,
  saving,
  error,
  onSave,
  onResetDefaults,
  updateReport,
  updateCheckBusy,
  updateActionBusy,
  updateActionResult,
  onCheckUpdates,
  onUpdateAction,
}: SettingsModalProps) {
  const [draft, setDraft] = useState<SpectraCraftSettings>(
    snapshot?.settings ?? DEFAULT_SPECTRACRAFT_SETTINGS,
  );
  const [savedFeedback, setSavedFeedback] = useState<string | null>(null);

  useEffect(() => {
    if (snapshot?.settings) {
      setDraft(snapshot.settings);
    }
  }, [snapshot, open]);

  useEffect(() => {
    if (!open || !focusSection) return;
    const timer = window.setTimeout(() => {
      const el = document.querySelector(`[data-testid="settings-section-${focusSection}"]`);
      if (el instanceof HTMLElement) {
        el.scrollIntoView({ behavior: 'smooth', block: 'start' });
      }
    }, 30);
    return () => window.clearTimeout(timer);
  }, [open, focusSection]);

  if (!open) return null;

  const handleSave = async () => {
    setSavedFeedback(null);
    const ok = await onSave(draft);
    if (ok) {
      setSavedFeedback('Saved to local craft-settings config.toml');
    }
  };

  const handleReset = async () => {
    setSavedFeedback(null);
    const ok = await onResetDefaults();
    if (ok) {
      setSavedFeedback('Restored documented ArtCraft defaults');
    }
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="SpectraCraft Settings and Updates"
      data-testid="settings-modal"
      className="fixed inset-0 z-50 bg-black/75 flex items-center justify-center p-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-[880px] max-h-[90vh] bg-[#14171f] border border-[#2f394c] shadow-2xl flex flex-col text-[#e5ecf7]"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="px-4 py-3 bg-[#1a1f2b] border-b border-[#2c3547] flex items-center justify-between gap-3 shrink-0">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <Settings className="w-4 h-4 text-[#5bb6ff] shrink-0" />
              <h2 className="text-[14px] font-semibold tracking-tight text-[#f4f7fc]">
                SpectraCraft Settings &amp; Updates
              </h2>
              <span className="font-mono text-[10.5px] text-[#7cc4fa]">
                · craft-settings schema v{snapshot?.schema_version ?? 1}
              </span>
            </div>
            <p className="text-[11px] font-mono text-[#8b98ae] mt-0.5 truncate">
              ArtCraft-aligned local preferences (uncommitted config.toml · preserves unknown suite keys)
            </p>
          </div>

          <button
            type="button"
            onClick={onClose}
            aria-label="Close Settings"
            className="h-7 w-7 bg-[#1f2634] hover:bg-[#2a3345] border border-[#38455c] flex items-center justify-center text-[#b7c4d9]"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {error && (
          <div className="px-4 py-2 bg-[#2d1619] border-b border-[#7a2e38] text-[#ffb4be] text-[11px] font-mono shrink-0">
            {error}
          </div>
        )}

        {savedFeedback && (
          <div
            data-testid="settings-saved-feedback"
            className="px-4 py-2 bg-[#132721] border-b border-[#2a6251] text-[#a5f0d8] text-[11px] font-mono flex items-center gap-2 shrink-0"
          >
            <CheckCircle2 className="w-3.5 h-3.5 text-[#45d1a9]" />
            <span>{savedFeedback}</span>
          </div>
        )}

        {/* Scrollable Content */}
        <div className="flex-1 overflow-y-auto p-4 space-y-4">
          {/* Section 1: ArtCraft Appearance & Report Viewer */}
          <section
            data-testid="settings-section-appearance"
            className="bg-[#181c26] border border-[#2b3344]"
          >
            <div className="px-3.5 py-2 bg-[#1d2330] border-b border-[#2b3344] flex items-center justify-between">
              <h3 className="text-[12px] font-semibold text-[#eef3fb]">
                1. ArtCraft Appearance &amp; Report Options
              </h3>
              <span className="font-mono text-[10px] text-[#8d9ab0]">[artcraft] · [report]</span>
            </div>
            <div className="p-3.5 grid grid-cols-1 md:grid-cols-2 gap-3 text-[11.5px]">
              <div className="space-y-1">
                <label
                  htmlFor="artcraft-theme"
                  className="block text-[11px] font-medium text-[#cdd8ea]"
                >
                  ArtCraft suite theme preference
                </label>
                <select
                  id="artcraft-theme"
                  data-testid="setting-artcraft-theme"
                  value={draft.artcraft.theme}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      artcraft: {
                        ...prev.artcraft,
                        theme: e.target.value as 'dark' | 'light',
                      },
                    }))
                  }
                  className="w-full h-7 px-2 bg-[#11151d] border border-[#354156] text-[#e6edf8] font-mono text-[11px]"
                >
                  <option value="dark">Dark Studio (Default)</option>
                  <option value="light">Light (Saved for ArtCraft suite compatibility)</option>
                </select>
              </div>

              <div className="space-y-1">
                <label
                  htmlFor="default-export-format"
                  className="block text-[11px] font-medium text-[#cdd8ea]"
                >
                  Default technical report export format
                </label>
                <select
                  id="default-export-format"
                  data-testid="setting-default-export-format"
                  value={draft.report.default_export_format}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      report: {
                        ...prev.report,
                        default_export_format: e.target.value as 'pdf' | 'markdown' | 'docx',
                      },
                    }))
                  }
                  className="w-full h-7 px-2 bg-[#11151d] border border-[#354156] text-[#e6edf8] font-mono text-[11px]"
                >
                  <option value="pdf">PDF (pinned craft-pdf + craft-atomic)</option>
                  <option value="markdown">Markdown (.md via craft-atomic)</option>
                  <option value="docx">DOCX (documented fallback)</option>
                </select>
              </div>

              <label className="flex items-start gap-2.5 cursor-pointer">
                <input
                  type="checkbox"
                  data-testid="setting-dark-mode-viewer"
                  checked={draft.report.dark_mode_viewer}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      report: { ...prev.report, dark_mode_viewer: e.target.checked },
                    }))
                  }
                  className="mt-0.5 accent-[#378ef0]"
                />
                <span>
                  <span className="font-medium text-[#e6edf8] block">
                    Dark-mode technical report viewer
                  </span>
                  <span className="text-[10.5px] text-[#8d99ad]">
                    High-contrast dark background for Before/After plots and white-paper text (default: ON).
                  </span>
                </span>
              </label>

              <label className="flex items-start gap-2.5 cursor-pointer">
                <input
                  type="checkbox"
                  checked={draft.report.auto_open_viewer}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      report: { ...prev.report, auto_open_viewer: e.target.checked },
                    }))
                  }
                  className="mt-0.5 accent-[#378ef0]"
                />
                <span>
                  <span className="font-medium text-[#e6edf8] block">
                    Auto-open report viewer after generation
                  </span>
                  <span className="text-[10.5px] text-[#8d99ad]">
                    Switches directly to the Report view once a blind report finishes generating.
                  </span>
                </span>
              </label>

              <label className="flex items-start gap-2.5 cursor-pointer md:col-span-2">
                <input
                  type="checkbox"
                  checked={draft.artcraft.auto_incorporate_shared_crates}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      artcraft: {
                        ...prev.artcraft,
                        auto_incorporate_shared_crates: e.target.checked,
                      },
                    }))
                  }
                  className="mt-0.5 accent-[#378ef0]"
                />
                <span>
                  <span className="font-medium text-[#e6edf8] block">
                    Adopt shared ArtCraft crates on accepted updates
                  </span>
                  <span className="text-[10.5px] text-[#8d99ad]">
                    Incorporates only already-pinned crates (craft-pdf, craft-atomic, craft-settings) behind thin wrappers; unused crates remain unlinked.
                  </span>
                </span>
              </label>
            </div>
          </section>

          {/* Section 2: Workspace & ArtCraft Layout ([workspace]) */}
          <section
            data-testid="settings-section-workspace"
            className="bg-[#181c26] border border-[#2b3344]"
          >
            <div className="px-3.5 py-2 bg-[#1d2330] border-b border-[#2b3344] flex items-center justify-between">
              <h3 className="text-[12px] font-semibold text-[#eef3fb]">
                2. Workspace &amp; ArtCraft Layout
              </h3>
              <span className="font-mono text-[10px] text-[#8d9ab0]">[workspace]</span>
            </div>
            <div className="p-3.5 grid grid-cols-1 md:grid-cols-2 gap-3 text-[11.5px]">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  data-testid="setting-ws-tool-rail"
                  checked={draft.workspace.show_tool_rail}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      workspace: { ...prev.workspace, show_tool_rail: e.target.checked },
                    }))
                  }
                  className="accent-[#378ef0]"
                />
                <span className="text-[#e6edf8]">Show Left Tool Palette (V / M / R / Z)</span>
              </label>

              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  data-testid="setting-ws-right-panel"
                  checked={draft.workspace.show_right_panel}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      workspace: { ...prev.workspace, show_right_panel: e.target.checked },
                    }))
                  }
                  className="accent-[#378ef0]"
                />
                <span className="text-[#e6edf8]">Show Right Tabbed Panel</span>
              </label>

              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={draft.workspace.show_waveform}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      workspace: { ...prev.workspace, show_waveform: e.target.checked },
                    }))
                  }
                  className="accent-[#378ef0]"
                />
                <span className="text-[#e6edf8]">Show Waveform View</span>
              </label>

              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={draft.workspace.show_spectrogram}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      workspace: { ...prev.workspace, show_spectrogram: e.target.checked },
                    }))
                  }
                  className="accent-[#378ef0]"
                />
                <span className="text-[#e6edf8]">Show Spectrogram View</span>
              </label>

              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={draft.workspace.show_transport}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      workspace: { ...prev.workspace, show_transport: e.target.checked },
                    }))
                  }
                  className="accent-[#378ef0]"
                />
                <span className="text-[#e6edf8]">Show Inline Transport Bar</span>
              </label>

              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={draft.workspace.show_diagnostics_dock}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      workspace: { ...prev.workspace, show_diagnostics_dock: e.target.checked },
                    }))
                  }
                  className="accent-[#378ef0]"
                />
                <span className="text-[#e6edf8]">Show Optional Bottom Diagnostics Dock</span>
              </label>

              <div className="space-y-1">
                <label htmlFor="ws-default-right-tab" className="block text-[11px] font-medium text-[#cdd8ea]">
                  Default Right Panel tab
                </label>
                <select
                  id="ws-default-right-tab"
                  value={draft.workspace.right_panel_tab}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      workspace: {
                        ...prev.workspace,
                        right_panel_tab: e.target.value as 'inspector' | 'diagnostics' | 'report' | 'room',
                      },
                    }))
                  }
                  className="w-full h-7 px-2 bg-[#11151d] border border-[#354156] text-[#e6edf8] font-mono text-[11px]"
                >
                  <option value="inspector">Inspector (5-Stage Pipeline)</option>
                  <option value="diagnostics">Diagnostics &amp; Dry Listening Gate</option>
                  <option value="report">Before/After Technical Report</option>
                  <option value="room">Room Views (2D Plans &amp; 3D Grid)</option>
                </select>
              </div>

              <div className="space-y-1">
                <label htmlFor="ws-spatial-mode" className="block text-[11px] font-medium text-[#cdd8ea]">
                  Room view layout mode
                </label>
                <select
                  id="ws-spatial-mode"
                  value={draft.workspace.spatial_mode}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      workspace: {
                        ...prev.workspace,
                        spatial_mode: e.target.value as 'plans' | '3d' | 'split',
                      },
                    }))
                  }
                  className="w-full h-7 px-2 bg-[#11151d] border border-[#354156] text-[#e6edf8] font-mono text-[11px]"
                >
                  <option value="plans">Plans (Top + Front)</option>
                  <option value="3d">3D Grid</option>
                  <option value="split">Split (Plans + 3D)</option>
                </select>
              </div>
            </div>
          </section>

          {/* Section 3: Update Checks & Staged Releases (relocated from top banner) */}
          <section
            data-testid="settings-section-updates"
            className="bg-[#181c26] border border-[#2b3344]"
          >
            <div className="px-3.5 py-2 bg-[#1d2330] border-b border-[#2b3344] flex flex-wrap items-center justify-between gap-2">
              <div>
                <h3 className="text-[12px] font-semibold text-[#eef3fb]">
                  3. Updates, Staged Releases &amp; Pinned Dependencies
                </h3>
                <p className="text-[10.5px] text-[#8d99ad]">
                  Quiet background check only — update controls live here and in Help, never as a bar under the title. Stage 1 physics stay untouched.
                </p>
              </div>
              <div className="flex items-center gap-1.5">
                <button
                  type="button"
                  disabled={updateCheckBusy}
                  data-testid="settings-check-updates-btn"
                  onClick={() => void onCheckUpdates(true)}
                  className="h-7 px-2.5 bg-[#203047] hover:bg-[#2a3f5e] disabled:opacity-45 border border-[#3a5985] text-[#dceaff] text-[11px] flex items-center gap-1.5"
                >
                  <RefreshCw className={`w-3.5 h-3.5 ${updateCheckBusy ? 'animate-spin' : ''}`} />
                  <span>{updateCheckBusy ? 'Checking…' : 'Check for Updates'}</span>
                </button>
              </div>
            </div>

            <div className="p-3.5 space-y-3">
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3 text-[11.5px]">
                <label className="flex items-start gap-2.5 cursor-pointer">
                  <input
                    type="checkbox"
                    data-testid="setting-check-on-launch"
                    checked={draft.updates.check_on_launch}
                    onChange={(e) =>
                      setDraft((prev) => ({
                        ...prev,
                        updates: { ...prev.updates, check_on_launch: e.target.checked },
                      }))
                    }
                    className="mt-0.5 accent-[#378ef0]"
                  />
                  <span>
                    <span className="font-medium text-[#e6edf8] block">
                      Check for updates on launch (quiet background)
                    </span>
                    <span className="text-[10.5px] text-[#8d99ad]">
                      Checks pinned storytold/craft-libs releases without blocking UI or audio.
                    </span>
                  </span>
                </label>

                <label className="flex items-start gap-2.5 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={draft.updates.check_direct_dependencies}
                    onChange={(e) =>
                      setDraft((prev) => ({
                        ...prev,
                        updates: { ...prev.updates, check_direct_dependencies: e.target.checked },
                      }))
                    }
                    className="mt-0.5 accent-[#378ef0]"
                  />
                  <span>
                    <span className="font-medium text-[#e6edf8] block">
                      Check direct Cargo.toml dependencies
                    </span>
                    <span className="text-[10.5px] text-[#8d99ad]">
                      Also compares non-optional crates.io pins (hound, rustfft, serde, serde_json).
                    </span>
                  </span>
                </label>
              </div>

              {updateReport && (
                <div className="space-y-2.5 pt-2 border-t border-[#262e3d]">
                  <div
                    data-testid="settings-update-controls"
                    className="flex flex-wrap items-center justify-between gap-2 bg-[#121721] border border-[#283246] px-3 py-2"
                  >
                    <div className="text-[11px] text-[#d8e3f4]">
                      <span className="font-mono text-[10px] text-[#7cc4fa] mr-2">
                        {updateReport.has_staged_download ? 'STAGED UPDATE' : 'STATUS'}
                      </span>
                      {updateReport.summary}
                    </div>
                    <div className="flex items-center gap-1.5">
                      <button
                        type="button"
                        data-testid="settings-update-now-btn"
                        disabled={updateActionBusy !== null}
                        onClick={() => void onUpdateAction('update_now')}
                        className="h-6 px-2.5 bg-[#2a6fc9] hover:bg-[#3380e3] disabled:opacity-45 border border-[#4a92ed] text-white text-[10.5px] font-medium flex items-center gap-1"
                      >
                        <ShieldCheck className="w-3 h-3" />
                        <span>
                          {updateActionBusy === 'update_now' ? 'Verifying Sweep…' : 'Update now'}
                        </span>
                      </button>
                      <button
                        type="button"
                        data-testid="settings-download-later-btn"
                        disabled={updateActionBusy !== null}
                        onClick={() => void onUpdateAction('download_later')}
                        className="h-6 px-2.5 bg-[#233146] hover:bg-[#2d405b] disabled:opacity-45 border border-[#3e5678] text-[#d6e4f7] text-[10.5px] flex items-center gap-1"
                      >
                        <Download className="w-3 h-3" />
                        <span>Download and update later</span>
                      </button>
                      <button
                        type="button"
                        data-testid="settings-not-now-btn"
                        disabled={updateActionBusy !== null}
                        onClick={() => void onUpdateAction('not_now')}
                        className="h-6 px-2 bg-[#1f2633] hover:bg-[#2b3446] disabled:opacity-45 border border-[#3a465c] text-[#b4c2d8] text-[10.5px]"
                      >
                        Not now
                      </button>
                    </div>
                  </div>

                  {updateActionResult && (
                    <div
                      data-testid="settings-update-action-result"
                      className={`px-3 py-2 border font-mono text-[10.5px] ${
                        updateActionResult.ok
                          ? 'bg-[#122620] border-[#2b6251] text-[#a7f1da]'
                          : 'bg-[#2c171b] border-[#78313b] text-[#ffb8c2]'
                      }`}
                    >
                      {updateActionResult.message}
                    </div>
                  )}

                  {/* Pinned craft-libs Table */}
                  <div className="overflow-x-auto">
                    <table className="w-full text-left border-collapse font-mono text-[10.5px]">
                      <thead>
                        <tr className="bg-[#12161e] border-b border-[#252d3c] text-[#8b98ae]">
                          <th className="py-1.5 px-2.5">Pinned craft-libs Crate</th>
                          <th className="py-1.5 px-2.5">Thin Wrapper</th>
                          <th className="py-1.5 px-2.5">Current Pin</th>
                          <th className="py-1.5 px-2.5">Upstream</th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-[#222938]">
                        {updateReport.craft_libs.map((c) => (
                          <tr key={c.crate_name}>
                            <td className="py-1.5 px-2.5 text-[#e4ecf8]">{c.crate_name}</td>
                            <td className="py-1.5 px-2.5 text-[#9eb0cc]">{c.wrapper_module}</td>
                            <td className="py-1.5 px-2.5 text-[#6cd4f8]">
                              {c.current_pin_kind}={c.current_pin.slice(0, 10)}
                            </td>
                            <td className="py-1.5 px-2.5 text-[#a7e9d3]">
                              {c.latest_tag ??
                                (c.latest_main_rev
                                  ? `main@${c.latest_main_rev.slice(0, 10)}`
                                  : 'offline / pinned')}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>

                  {/* Direct Cargo.toml Dependencies Table */}
                  <div className="overflow-x-auto">
                    <table className="w-full text-left border-collapse font-mono text-[10.5px]">
                      <thead>
                        <tr className="bg-[#12161e] border-b border-[#252d3c] text-[#8b98ae]">
                          <th className="py-1.5 px-2.5">Direct Cargo.toml Crate</th>
                          <th className="py-1.5 px-2.5">Pinned</th>
                          <th className="py-1.5 px-2.5">Latest Stable</th>
                          <th className="py-1.5 px-2.5">Status</th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-[#222938]">
                        {[...updateReport.direct_deps]
                          .sort((a, b) => {
                            if (a.update_available !== b.update_available) {
                              return a.update_available ? -1 : 1;
                            }
                            if (a.optional !== b.optional) {
                              return a.optional ? 1 : -1;
                            }
                            return a.crate_name.localeCompare(b.crate_name);
                          })
                          .map((d) => (
                            <tr key={d.crate_name}>
                              <td className="py-1.5 px-2.5 text-[#e4ecf8]">
                                {d.crate_name}
                                {d.optional && (
                                  <span className="ml-1.5 text-[9.5px] text-[#7f8ba1]">(optional)</span>
                                )}
                              </td>
                              <td className="py-1.5 px-2.5 text-[#9eb0cc]">{d.current_version}</td>
                              <td className="py-1.5 px-2.5 text-[#dce6f5]">
                                {d.latest_version ?? '—'}
                              </td>
                              <td
                                className={`py-1.5 px-2.5 ${
                                  d.update_available ? 'text-[#f5bf5b]' : 'text-[#8ea1bd]'
                                }`}
                              >
                                {d.status}
                              </td>
                            </tr>
                          ))}
                      </tbody>
                    </table>
                  </div>

                  <p className="text-[10px] font-mono text-[#7f8ca3]">
                    Shared ArtCraft incorporation: {updateReport.incorporation_policy}
                  </p>
                </div>
              )}
            </div>
          </section>

          {/* Section 4: Local Paths & Environment Metadata (No Raw Secrets) */}
          <section
            data-testid="settings-section-paths"
            className="bg-[#181c26] border border-[#2b3344]"
          >
            <div className="px-3.5 py-2 bg-[#1d2330] border-b border-[#2b3344] flex items-center justify-between">
              <div className="flex items-center gap-1.5">
                <FolderCog className="w-3.5 h-3.5 text-[#6cd4f8]" />
                <h3 className="text-[12px] font-semibold text-[#eef3fb]">
                  4. Local Paths &amp; Environment Reference
                </h3>
              </div>
              <span className="font-mono text-[10px] text-[#8d9ab0]">[paths] · [integration]</span>
            </div>
            <div className="p-3.5 grid grid-cols-1 md:grid-cols-2 gap-3 text-[11px]">
              <div className="space-y-1">
                <label htmlFor="last-input-dir" className="block text-[#cdd8ea] font-medium">
                  Default input folder (local only)
                </label>
                <input
                  id="last-input-dir"
                  type="text"
                  value={draft.paths.last_input_dir}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      paths: { ...prev.paths, last_input_dir: e.target.value },
                    }))
                  }
                  placeholder="/path/to/raw-recordings"
                  className="w-full h-7 px-2 bg-[#11151d] border border-[#354156] text-[#e6edf8] font-mono text-[11px]"
                />
              </div>

              <div className="space-y-1">
                <label htmlFor="last-export-dir" className="block text-[#cdd8ea] font-medium">
                  Default export folder (local only)
                </label>
                <input
                  id="last-export-dir"
                  type="text"
                  value={draft.paths.last_export_dir}
                  onChange={(e) =>
                    setDraft((prev) => ({
                      ...prev,
                      paths: { ...prev.paths, last_export_dir: e.target.value },
                    }))
                  }
                  placeholder="/path/to/exports"
                  className="w-full h-7 px-2 bg-[#11151d] border border-[#354156] text-[#e6edf8] font-mono text-[11px]"
                />
              </div>

              <div className="md:col-span-2 bg-[#12161f] border border-[#262e3d] px-3 py-2 flex flex-wrap items-center justify-between gap-2 font-mono text-[10.5px]">
                <div className="flex flex-wrap items-center gap-2 text-[#a5b3ca]">
                  <label htmlFor="ai-assist-env-var">Optional AI-assist env var name:</label>
                  <input
                    id="ai-assist-env-var"
                    type="text"
                    value={draft.integration.ai_assist_env_var}
                    onChange={(e) =>
                      setDraft((prev) => ({
                        ...prev,
                        integration: { ...prev.integration, ai_assist_env_var: e.target.value },
                      }))
                    }
                    className="h-6 px-2 w-40 bg-[#0d1118] border border-[#354156] text-[#eef4fc] font-mono text-[10.5px]"
                  />
                  <span className="text-[#808ea6]">(secrets never saved in config.toml)</span>
                </div>
                <span className="text-[10.5px] text-[#9eb0cc]">
                  {snapshot?.ai_assist_key_present ? 'Present in host env' : 'Not set in host env'}
                </span>
              </div>
            </div>
          </section>
        </div>

        {/* Footer */}
        <div className="px-4 py-2.5 bg-[#181d28] border-t border-[#2c3547] flex flex-wrap items-center justify-between gap-3 shrink-0">
          <div className="font-mono text-[10px] text-[#8391a8] truncate max-w-[440px]" title={snapshot?.config_path ?? ''}>
            Store: {snapshot?.config_path ?? 'local uncommitted config.toml'} ({snapshot?.load_source ?? 'Auto'})
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              disabled={saving}
              data-testid="settings-reset-btn"
              onClick={() => void handleReset()}
              className="h-7 px-2.5 bg-[#232836] hover:bg-[#2f3649] disabled:opacity-45 border border-[#3d475e] text-[#d1dced] text-[11px] flex items-center gap-1.5"
            >
              <RotateCcw className="w-3.5 h-3.5" />
              <span>Reset to Defaults</span>
            </button>

            <button
              type="button"
              disabled={saving}
              data-testid="settings-save-btn"
              onClick={() => void handleSave()}
              className="h-7 px-3 bg-[#378ef0] hover:bg-[#2c7ad4] disabled:opacity-45 text-white font-medium text-[11px]"
            >
              {saving ? 'Saving…' : 'Save Settings'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
