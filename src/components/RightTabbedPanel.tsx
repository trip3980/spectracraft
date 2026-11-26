import React, { useState } from 'react';
import {
  Activity,
  Box,
  Check,
  Copy,
  Download,
  FileText,
  Maximize2,
  RefreshCw,
  ShieldCheck,
  SlidersHorizontal,
  X,
} from 'lucide-react';
import type { RightPanelTab } from '../lib/settingsAndUpdates.ts';
import { formatBytes, type ReportExportResponse, type TechnicalReport } from '../lib/technicalReport.ts';
import {
  FindingsTable,
  SpectrogramCanvas,
  ThirdOctaveCanvas,
  UpperBandCanvas,
  WaveformComparisonCanvas,
} from './TechnicalReportPlots.tsx';

interface RightTabbedPanelProps {
  activeTab: RightPanelTab;
  onTabChange: (tab: RightPanelTab) => void;
  onHidePanel: () => void;
  inspectorContent: React.ReactNode;
  diagnosticsContent: React.ReactNode;
  roomContent: React.ReactNode;
  // Report tab props
  hasSession: boolean;
  report: TechnicalReport | null;
  reportBusy: boolean;
  reportError: string | null;
  onGenerateReport: () => void;
  onExpandReportModal: () => void;
  onExportReport: (format: 'pdf' | 'markdown' | 'docx') => Promise<ReportExportResponse | null>;
  reportExportBusy: 'pdf' | 'markdown' | 'docx' | null;
  reportLastExport: ReportExportResponse | null;
  defaultExportFormat?: 'pdf' | 'markdown' | 'docx';
  darkMode?: boolean;
}

type DockReportSubTab = 'plots' | 'findings' | 'markdown';

const TABS: Array<{ id: RightPanelTab; label: string; icon: React.ReactNode }> = [
  { id: 'inspector', label: 'Inspector', icon: <SlidersHorizontal className="w-3.5 h-3.5" /> },
  { id: 'diagnostics', label: 'Diagnostics', icon: <Activity className="w-3.5 h-3.5" /> },
  { id: 'report', label: 'Report', icon: <FileText className="w-3.5 h-3.5" /> },
  { id: 'room', label: 'Room', icon: <Box className="w-3.5 h-3.5" /> },
];

export function RightTabbedPanel({
  activeTab,
  onTabChange,
  onHidePanel,
  inspectorContent,
  diagnosticsContent,
  roomContent,
  hasSession,
  report,
  reportBusy,
  reportError,
  onGenerateReport,
  onExpandReportModal,
  onExportReport,
  reportExportBusy,
  reportLastExport,
  defaultExportFormat = 'pdf',
  darkMode = true,
}: RightTabbedPanelProps) {
  const [reportSubTab, setReportSubTab] = useState<DockReportSubTab>('plots');
  const [copiedMd, setCopiedMd] = useState(false);

  const handleCopyMarkdown = async () => {
    if (!report) return;
    try {
      await navigator.clipboard.writeText(report.whitepaper_markdown);
      setCopiedMd(true);
      window.setTimeout(() => setCopiedMd(false), 1800);
    } catch {
      // Clipboard API may be blocked in restricted contexts; ignore quietly.
    }
  };

  return (
    <aside
      aria-label="Right studio panels"
      data-testid="right-tabbed-panel"
      className="h-full w-full bg-[#222429] border-l border-[#383b44] flex flex-col min-h-0 min-w-0"
    >
      {/* ArtCraft / Adobe-style Panel Tab Strip */}
      <div
        role="tablist"
        aria-label="Right panel tabs"
        className="h-8 bg-[#282a30] border-b border-[#3a3d47] flex items-center justify-between px-1 shrink-0"
      >
        <div className="flex items-center h-full min-w-0">
          {TABS.map((tab) => {
            const isActive = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                type="button"
                role="tab"
                aria-selected={isActive}
                data-testid={`right-tab-${tab.id}`}
                onClick={() => onTabChange(tab.id)}
                className={`h-full px-2.5 text-[11px] font-medium flex items-center gap-1.5 whitespace-nowrap shrink-0 border-r border-[#363942] transition-colors ${
                  isActive
                    ? 'bg-[#1d1f24] text-[#f3f6fb] border-t-2 border-t-[#378ef0]'
                    : 'text-[#a6aebc] hover:text-[#eef2f9] hover:bg-[#30333b]'
                }`}
              >
                {tab.icon}
                <span>{tab.label}</span>
              </button>
            );
          })}
        </div>

        <button
          type="button"
          onClick={onHidePanel}
          title="Hide Right Tabbed Panel (re-open from Workspace menu)"
          aria-label="Hide Right Tabbed Panel"
          className="h-6 w-6 flex items-center justify-center text-[#949ca9] hover:text-white hover:bg-[#363942] shrink-0"
        >
          <X className="w-3.5 h-3.5" />
        </button>
      </div>

      {/* Tab Body — never hides or destroys the main audio canvas */}
      <div className="flex-1 min-h-0 flex flex-col overflow-hidden">
        {activeTab === 'inspector' && (
          <div data-testid="right-panel-inspector" className="flex-1 min-h-0 flex flex-col">
            {inspectorContent}
          </div>
        )}

        {activeTab === 'diagnostics' && (
          <div data-testid="right-panel-diagnostics" className="flex-1 min-h-0 flex flex-col">
            {diagnosticsContent}
          </div>
        )}

        {activeTab === 'room' && (
          <div data-testid="right-panel-room" className="flex-1 min-h-0 flex flex-col bg-[#1c1d20]">
            {roomContent}
          </div>
        )}

        {activeTab === 'report' && (
          <div
            data-testid="right-panel-report"
            data-theme={darkMode ? 'dark' : 'light'}
            className={`flex-1 min-h-0 overflow-y-auto p-3 space-y-3 text-[11.5px] ${
              darkMode ? 'bg-[#0f1218] text-[#e4ebf7]' : 'bg-[#f4f6fa] text-[#172030]'
            }`}
          >
            <div
              className={`flex items-start justify-between gap-2 pb-2 border-b ${
                darkMode ? 'border-[#252d3e]' : 'border-[#d0d7e3]'
              }`}
            >
              <div className="min-w-0">
                <div className="flex items-center gap-1.5">
                  <h3
                    className={`text-[12px] font-semibold truncate ${
                      darkMode ? 'text-[#f3f6fc]' : 'text-[#111827]'
                    }`}
                  >
                    Before / After Technical Report
                  </h3>
                  <span
                    className={`px-1.5 py-0.5 border font-mono text-[8.5px] shrink-0 ${
                      darkMode
                        ? 'bg-[#122a24] border-[#2d705e] text-[#5fe0b7]'
                        : 'bg-[#e6f4ef] border-[#4ca68a] text-[#145946]'
                    }`}
                  >
                    {darkMode ? 'BLIND · DARK' : 'BLIND · LIGHT'}
                  </span>
                </div>
                <p
                  className={`text-[10px] mt-0.5 font-mono truncate ${
                    darkMode ? 'text-[#8d9ab0]' : 'text-[#526075]'
                  }`}
                >
                  {report
                    ? `${(report.before.sample_rate / 1000).toFixed(1)} kHz · ${report.before.duration_s.toFixed(2)}s · Parseval ${report.before.parseval_verified && report.after.parseval_verified ? 'PASS' : 'CHECK'}`
                    : 'Independent audit of raw input vs. final export'}
                </p>
              </div>
              {report && (
                <button
                  type="button"
                  data-testid="right-report-expand-btn"
                  onClick={onExpandReportModal}
                  className="h-6 px-2 bg-[#1f2c3f] hover:bg-[#2a3c55] border border-[#3a557a] text-[#d6e7ff] text-[10.5px] flex items-center gap-1 whitespace-nowrap shrink-0"
                  title="Open full-screen viewer with wide side-by-side white paper"
                >
                  <Maximize2 className="w-3 h-3" />
                  <span>Full View</span>
                </button>
              )}
            </div>

            <div className="flex flex-wrap items-center gap-1.5">
              <button
                type="button"
                disabled={!hasSession || reportBusy}
                data-testid="right-report-generate-btn"
                onClick={onGenerateReport}
                className="h-7 px-2.5 bg-[#2f6ec4] hover:bg-[#387dd9] disabled:opacity-40 text-white text-[11px] font-medium flex items-center gap-1.5"
              >
                <RefreshCw className={`w-3.5 h-3.5 ${reportBusy ? 'animate-spin' : ''}`} />
                <span>
                  {reportBusy
                    ? 'Auditing…'
                    : report
                      ? 'Re-run Blind Audit'
                      : 'Generate Blind Report'}
                </span>
              </button>

              {report && (
                <>
                  <button
                    type="button"
                    disabled={reportExportBusy !== null}
                    data-testid="right-report-export-pdf-btn"
                    onClick={() => void onExportReport('pdf')}
                    className={`h-7 px-2 border text-[11px] flex items-center gap-1 disabled:opacity-40 ${
                      defaultExportFormat === 'pdf'
                        ? 'bg-[#1b304a] border-[#3d74b8] text-[#dcebff]'
                        : 'bg-[#1c212c] border-[#333c4f] text-[#c7d3e6] hover:bg-[#252c3b]'
                    }`}
                    title="Export multi-page dark-mode PDF with embedded plots via pinned craft-pdf"
                  >
                    <Download className="w-3 h-3" />
                    <span>{reportExportBusy === 'pdf' ? 'PDF…' : 'PDF (craft-pdf)'}</span>
                  </button>
                  <button
                    type="button"
                    disabled={reportExportBusy !== null}
                    data-testid="right-report-export-md-btn"
                    onClick={() => void onExportReport('markdown')}
                    className="h-7 px-2 bg-[#1c212c] hover:bg-[#252c3b] border border-[#333c4f] text-[#c7d3e6] text-[11px] disabled:opacity-40"
                  >
                    {reportExportBusy === 'markdown' ? 'MD…' : 'MD'}
                  </button>
                  <button
                    type="button"
                    disabled={reportExportBusy !== null}
                    data-testid="right-report-export-docx-btn"
                    onClick={() => void onExportReport('docx')}
                    className="h-7 px-2 bg-[#1c212c] hover:bg-[#252c3b] border border-[#333c4f] text-[#c7d3e6] text-[11px] disabled:opacity-40"
                  >
                    {reportExportBusy === 'docx' ? 'DOCX…' : 'DOCX'}
                  </button>
                  <button
                    type="button"
                    data-testid="right-report-copy-md-btn"
                    onClick={() => void handleCopyMarkdown()}
                    className="h-7 px-2 bg-[#1c212c] hover:bg-[#252c3b] border border-[#333c4f] text-[#c7d3e6] text-[11px] flex items-center gap-1"
                    title="Copy concise technical white-paper Markdown to clipboard"
                  >
                    {copiedMd ? (
                      <>
                        <Check className="w-3 h-3 text-[#5fe0b7]" />
                        <span className="text-[#5fe0b7]">Copied</span>
                      </>
                    ) : (
                      <>
                        <Copy className="w-3 h-3" />
                        <span>Copy</span>
                      </>
                    )}
                  </button>
                </>
              )}
            </div>

            {reportError && (
              <div className="p-2.5 bg-[#2d1619] border border-[#7a2e38] text-[#ffb4be] font-mono text-[10.5px]">
                {reportError}
              </div>
            )}

            {reportLastExport && (
              <div
                data-testid="right-report-export-receipt"
                className="p-2.5 bg-[#122620] border border-[#2b6251] text-[#a7f1da] font-mono text-[10px] space-y-1"
              >
                <div>
                  Exported {reportLastExport.filename} ({formatBytes(reportLastExport.receipt.bytes)})
                  via {reportLastExport.receipt.engine}
                </div>
                {reportLastExport.receipt.fallback_used && reportLastExport.receipt.fallback_note && (
                  <div className="text-[#f5bf5b]">
                    Fallback: {reportLastExport.receipt.fallback_note}
                  </div>
                )}
                {reportLastExport.downloadUrl && (
                  <a
                    href={reportLastExport.downloadUrl}
                    download={reportLastExport.filename}
                    className="underline text-[#7fe6c7] hover:text-white inline-block"
                  >
                    Download {reportLastExport.filename}
                  </a>
                )}
              </div>
            )}

            {!hasSession ? (
              <div className="py-8 text-center text-[#8894a8] font-mono text-[11px] border border-[#252d3c] bg-[#161a24] p-4">
                Open a single-source WAV or Reference session to run the blind Before/After technical audit.
              </div>
            ) : !report ? (
              <div className="py-6 text-[#9aa7bd] border border-[#252d3c] bg-[#161a24] p-3.5 space-y-1.5">
                <div className="font-medium text-[#dde6f5]">Ready for blind comparison</div>
                <p className="text-[11px] leading-relaxed">
                  Click <strong>Generate Blind Report</strong> to measure raw input and final export
                  independently, render real-buffer waveform &amp; spectrogram plots right here in
                  dark mode, and export via <code>craft-pdf</code>.
                </p>
              </div>
            ) : (
              <div className="space-y-3">
                {/* Embedded Viewer Sub-Tabs */}
                <div
                  role="tablist"
                  aria-label="Workspace report sections"
                  className="flex items-center gap-1 border-b border-[#252d3e] pb-2"
                >
                  {(
                    [
                      ['plots', 'Deltas & Plots'],
                      ['findings', `Before / After (${report.before.findings.length + report.after.findings.length})`],
                      ['markdown', 'White Paper'],
                    ] as [DockReportSubTab, string][]
                  ).map(([id, label]) => (
                    <button
                      key={id}
                      type="button"
                      role="tab"
                      aria-selected={reportSubTab === id}
                      data-testid={`ws-report-subtab-${id}`}
                      onClick={() => setReportSubTab(id)}
                      className={`px-2 py-1 text-[10.5px] border transition-colors ${
                        reportSubTab === id
                          ? 'bg-[#21334d] border-[#3d78c2] text-[#f2f7ff] font-medium'
                          : 'bg-[#151a24] border-[#273042] text-[#96a4ba] hover:text-[#e1e9f5]'
                      }`}
                    >
                      {label}
                    </button>
                  ))}
                </div>

                {reportSubTab === 'plots' && (
                  <div className="space-y-3">
                    {/* Comparison Summary Deltas */}
                    <section className="bg-[#151820] border border-[#283142]">
                      <div className="px-3 py-1.5 bg-[#1b202b] border-b border-[#283142] flex items-center justify-between">
                        <span className="font-semibold text-[11px] text-[#edf2fa]">
                          Comparison Summary (Numeric Deltas)
                        </span>
                        <span className="font-mono text-[10px] text-[#5fe0b7] flex items-center gap-1">
                          <ShieldCheck className="w-3 h-3" />
                          {report.comparison.deltas.length} items
                        </span>
                      </div>
                      <div className="divide-y divide-[#222938]">
                        {report.comparison.deltas.length === 0 ? (
                          <div className="px-3 py-2 text-[#8d9ab0] text-[11px]">
                            No significant deltas exceeded threshold.
                          </div>
                        ) : (
                          report.comparison.deltas.map((d, idx) => (
                            <div key={`${d.metric}-${idx}`} className="px-3 py-1.5 space-y-0.5">
                              <div className="flex items-center justify-between gap-2">
                                <span className="text-[#d6e0f0] font-medium text-[11px]">
                                  {d.metric}
                                </span>
                                <span className="font-mono text-[10px] text-[#6cd4f8] tabular-nums shrink-0">
                                  {d.delta_numeric !== null
                                    ? `${d.delta_numeric > 0 ? '+' : ''}${d.delta_numeric.toFixed(1)}${d.unit ? ` ${d.unit}` : ''}`
                                    : d.summary}
                                </span>
                              </div>
                              <div className="font-mono text-[10px] text-[#8d99ad] tabular-nums">
                                {d.before_value} → {d.after_value}
                              </div>
                            </div>
                          ))
                        )}
                      </div>
                    </section>

                    {/* Real-Buffer Dark-Mode Plots (Shared Scales) */}
                    <div className="space-y-2.5">
                      <WaveformComparisonCanvas
                        plots={report.plots}
                        compact
                        testId="ws-report-waveform-canvas"
                      />
                      <SpectrogramCanvas
                        title="SPECTROGRAM — ORIGINAL"
                        grid={report.plots.spectrogram_before_db}
                        cols={report.plots.spectrogram_cols}
                        rows={report.plots.spectrogram_rows}
                        durationS={report.plots.duration_s}
                        maxHz={report.plots.max_hz}
                        freqHz={report.plots.spectrogram_freq_hz}
                        mode="magnitude"
                        dbRange={report.plots.spectrogram_db_range}
                        diffMaxAbs={report.plots.diff_db_range[1]}
                        compact
                        testId="ws-report-spec-before-canvas"
                      />
                      <SpectrogramCanvas
                        title="SPECTROGRAM — FINAL (SAME SCALE)"
                        grid={report.plots.spectrogram_after_db}
                        cols={report.plots.spectrogram_cols}
                        rows={report.plots.spectrogram_rows}
                        durationS={report.plots.duration_s}
                        maxHz={report.plots.max_hz}
                        freqHz={report.plots.spectrogram_freq_hz}
                        mode="magnitude"
                        dbRange={report.plots.spectrogram_db_range}
                        diffMaxAbs={report.plots.diff_db_range[1]}
                        compact
                        testId="ws-report-spec-after-canvas"
                      />
                      <SpectrogramCanvas
                        title="DIFFERENCE (FINAL − ORIGINAL)"
                        grid={report.plots.spectrogram_diff_db}
                        cols={report.plots.spectrogram_cols}
                        rows={report.plots.spectrogram_rows}
                        durationS={report.plots.duration_s}
                        maxHz={report.plots.max_hz}
                        freqHz={report.plots.spectrogram_freq_hz}
                        mode="diff"
                        dbRange={report.plots.spectrogram_db_range}
                        diffMaxAbs={report.plots.diff_db_range[1]}
                        compact
                        testId="ws-report-spec-diff-canvas"
                      />
                      <ThirdOctaveCanvas
                        plots={report.plots}
                        compact
                        testId="ws-report-third-octave-canvas"
                      />
                      <UpperBandCanvas
                        plots={report.plots}
                        compact
                        testId="ws-report-upper-band-canvas"
                      />
                    </div>
                  </div>
                )}

                {reportSubTab === 'findings' && (
                  <div className="space-y-3" data-testid="ws-report-full-findings">
                    <FindingsTable
                      title="Before Analysis (Blind)"
                      subtitle="Raw input only — zero access to final export"
                      scopeBadge={report.before.blind_scope}
                      findings={report.before.findings}
                      compact
                    />
                    <FindingsTable
                      title="After Analysis (Blind)"
                      subtitle="Final export only — zero access to Before report"
                      scopeBadge={report.after.blind_scope}
                      findings={report.after.findings}
                      compact
                    />
                    <section className="bg-[#131720] border border-[#262e3d] p-2.5 text-[10px] font-mono text-[#8e9bb2] space-y-1">
                      <div className="text-[#c8d5ea] font-semibold">craft-libs Integration</div>
                      <div>
                        <span className="text-[#5fe0b7]">Used:</span>{' '}
                        {report.craft_libs.crates_used.join(' · ')}
                      </div>
                      <div>
                        <span className="text-[#f5bf5b]">Fallbacks:</span>{' '}
                        {report.craft_libs.unavailable_fallbacks.join(' ')}
                      </div>
                    </section>
                  </div>
                )}

                {reportSubTab === 'markdown' && (
                  <pre
                    data-testid="ws-report-markdown"
                    className="bg-[#12161f] border border-[#283142] p-3 font-mono text-[10px] leading-relaxed text-[#dce5f4] whitespace-pre-wrap overflow-x-auto select-text"
                  >
                    {report.whitepaper_markdown}
                  </pre>
                )}
              </div>
            )}
          </div>
        )}
      </div>
    </aside>
  );
}
