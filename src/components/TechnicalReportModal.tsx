import { useState } from 'react';
import {
  Activity,
  Check,
  CheckCircle2,
  Copy,
  Download,
  FileText,
  RefreshCw,
  ShieldCheck,
  X,
} from 'lucide-react';
import {
  formatBytes,
  type ReportExportResponse,
  type TechnicalReport,
} from '../lib/technicalReport.ts';
import {
  FindingsTable,
  SpectrogramCanvas,
  ThirdOctaveCanvas,
  UpperBandCanvas,
  WaveformComparisonCanvas,
} from './TechnicalReportPlots.tsx';

interface TechnicalReportModalProps {
  open: boolean;
  onClose: () => void;
  report: TechnicalReport | null;
  busy: boolean;
  error: string | null;
  onRegenerate: () => void;
  onExport: (format: 'pdf' | 'markdown' | 'docx') => Promise<ReportExportResponse | null>;
  exportBusyFormat: 'pdf' | 'markdown' | 'docx' | null;
  lastExport: ReportExportResponse | null;
  darkMode?: boolean;
  defaultExportFormat?: 'pdf' | 'markdown' | 'docx';
}

type ViewerTab = 'whitepaper' | 'plots' | 'markdown';


export function TechnicalReportModal({
  open,
  onClose,
  report,
  busy,
  error,
  onRegenerate,
  onExport,
  exportBusyFormat,
  lastExport,
  darkMode = true,
  defaultExportFormat = 'pdf',
}: TechnicalReportModalProps) {
  const [activeTab, setActiveTab] = useState<ViewerTab>('whitepaper');
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

  if (!open) return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Before and After Technical Report Viewer"
      data-testid="technical-report-viewer"
      data-theme={darkMode ? 'dark' : 'light'}
      className="fixed inset-0 z-50 bg-black/75 flex items-center justify-center p-4"
      onClick={onClose}
    >
      <div
        className={`w-full max-w-[1060px] max-h-[92vh] border shadow-2xl flex flex-col ${
          darkMode
            ? 'bg-[#0f1218] border-[#2c3547] text-[#e8eef8]'
            : 'bg-[#f4f7fb] border-[#bcc8da] text-[#151c27]'
        }`}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Top White-Paper Header Bar */}
        <div
          className={`px-4 py-3 border-b flex flex-wrap items-center justify-between gap-3 shrink-0 ${
            darkMode
              ? 'bg-[#151a24] border-[#2b3446]'
              : 'bg-[#e8eef6] border-[#c5d1e2]'
          }`}
        >
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <FileText className={`w-4 h-4 shrink-0 ${darkMode ? 'text-[#4bc3f2]' : 'text-[#1c6cb0]'}`} />
              <h2
                className={`text-[14px] font-semibold tracking-tight truncate ${
                  darkMode ? 'text-[#f4f7fc]' : 'text-[#101824]'
                }`}
              >
                Technical White Paper — Before &amp; After Analysis
              </h2>
              <span
                className={`px-1.5 py-0.5 border font-mono text-[9.5px] ${
                  darkMode
                    ? 'bg-[#122a24] border-[#2d705e] text-[#5fe0b7]'
                    : 'bg-[#dff5ee] border-[#5ba88f] text-[#145e49]'
                }`}
              >
                {darkMode ? 'DARK MODE · BLIND PROTOCOL' : 'LIGHT MODE · BLIND PROTOCOL'}
              </span>
            </div>
            <p
              className={`text-[11px] font-mono mt-0.5 truncate ${
                darkMode ? 'text-[#8d9ab0]' : 'text-[#4c5b73]'
              }`}
            >
              {report
                ? `${report.title} · ${report.pipeline_label} · ${(report.before.sample_rate / 1000).toFixed(1)} kHz mono · ${report.before.duration_s.toFixed(3)} s`
                : 'Blind raw-input vs final-export acoustic audit'}
            </p>
          </div>

          <div className="flex items-center gap-1.5 shrink-0">
            <button
              type="button"
              disabled={busy}
              onClick={onRegenerate}
              className="h-7 px-2.5 bg-[#1f2634] hover:bg-[#283245] disabled:opacity-40 border border-[#38455c] text-[11px] text-[#dce6f5] flex items-center gap-1.5"
            >
              <RefreshCw className={`w-3.5 h-3.5 ${busy ? 'animate-spin' : ''}`} />
              <span>Re-run Blind Audit</span>
            </button>

            <button
              type="button"
              disabled={!report || exportBusyFormat !== null}
              onClick={() => void onExport('pdf')}
              className={`h-7 px-2.5 disabled:opacity-40 border text-[11px] flex items-center gap-1.5 ${
                defaultExportFormat === 'pdf'
                  ? 'bg-[#2468b8] hover:bg-[#2c7ad4] border-[#3a83d8] font-medium text-white'
                  : 'bg-[#1e2430] hover:bg-[#2a3345] border-[#38455c] text-[#d6e0f0]'
              }`}
              title="Export dark-mode PDF with embedded plots via pinned craft-pdf"
            >
              <Download className="w-3.5 h-3.5" />
              <span>{exportBusyFormat === 'pdf' ? 'Exporting PDF…' : 'Export PDF (craft-pdf)'}</span>
            </button>

            <button
              type="button"
              disabled={!report || exportBusyFormat !== null}
              onClick={() => void onExport('markdown')}
              className={`h-7 px-2.5 disabled:opacity-40 border text-[11px] flex items-center gap-1.5 ${
                defaultExportFormat === 'markdown'
                  ? 'bg-[#2468b8] hover:bg-[#2c7ad4] border-[#3a83d8] font-medium text-white'
                  : 'bg-[#1e2430] hover:bg-[#2a3345] border-[#38455c] text-[#d6e0f0]'
              }`}
              title="Export technical white-paper Markdown"
            >
              <span>{exportBusyFormat === 'markdown' ? 'Exporting…' : 'Export MD'}</span>
            </button>

            <button
              type="button"
              disabled={!report || exportBusyFormat !== null}
              onClick={() => void onExport('docx')}
              className={`h-7 px-2.5 disabled:opacity-40 border text-[11px] flex items-center gap-1.5 ${
                defaultExportFormat === 'docx'
                  ? 'bg-[#2468b8] hover:bg-[#2c7ad4] border-[#3a83d8] font-medium text-white'
                  : 'bg-[#1e2430] hover:bg-[#2a3345] border-[#38455c] text-[#d6e0f0]'
              }`}
              title="Export Word DOCX (uses documented Markdown fallback if craft-docx is unavailable offline)"
            >
              <span>{exportBusyFormat === 'docx' ? 'Exporting…' : 'Export DOCX'}</span>
            </button>

            <button
              type="button"
              disabled={!report}
              data-testid="modal-report-copy-md-btn"
              onClick={() => void handleCopyMarkdown()}
              className="h-7 px-2.5 disabled:opacity-40 bg-[#1e2430] hover:bg-[#2a3345] border border-[#38455c] text-[11px] text-[#d6e0f0] flex items-center gap-1.5"
              title="Copy full technical white-paper Markdown to clipboard"
            >
              {copiedMd ? (
                <>
                  <Check className="w-3.5 h-3.5 text-[#5fe0b7]" />
                  <span className="text-[#5fe0b7]">Copied MD</span>
                </>
              ) : (
                <>
                  <Copy className="w-3.5 h-3.5" />
                  <span>Copy MD</span>
                </>
              )}
            </button>

            <button
              type="button"
              onClick={onClose}
              aria-label="Close Technical Report Viewer"
              className="h-7 w-7 bg-[#1d222e] hover:bg-[#2b3344] border border-[#343f54] flex items-center justify-center text-[#b7c4d9]"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* Sub-navigation & Export Status Banner */}
        <div className="px-4 py-2 bg-[#12161f] border-b border-[#242c3c] flex flex-wrap items-center justify-between gap-2 shrink-0">
          <div className="flex items-center gap-1">
            {(
              [
                ['whitepaper', 'White Paper & Plots'],
                ['plots', 'Audio Plots Only'],
                ['markdown', 'Raw White-Paper Markdown'],
              ] as [ViewerTab, string][]
            ).map(([id, label]) => (
              <button
                key={id}
                type="button"
                onClick={() => setActiveTab(id)}
                className={`px-2.5 py-1 text-[11px] border ${
                  activeTab === id
                    ? 'bg-[#21334d] border-[#3d78c2] text-[#f2f7ff] font-medium'
                    : 'bg-[#171c26] border-[#2a3345] text-[#98a6bc] hover:text-[#e1e9f5]'
                }`}
              >
                {label}
              </button>
            ))}
          </div>

          {report && (
            <div className="flex items-center gap-3 font-mono text-[10px] text-[#8d9bb2]">
              <span className="flex items-center gap-1 text-[#5fe0b7]">
                <ShieldCheck className="w-3.5 h-3.5" />
                Before &amp; After Blind-Isolated
              </span>
              <span>
                Parseval: {report.before.parseval_verified && report.after.parseval_verified ? 'PASS' : 'CHECK'}
              </span>
            </div>
          )}
        </div>

        {lastExport && (
          <div className="px-4 py-2 bg-[#132620] border-b border-[#295c4d] flex flex-wrap items-center justify-between gap-2 text-[11px] shrink-0">
            <div className="flex items-center gap-2 text-[#a8f0d8] min-w-0">
              <CheckCircle2 className="w-4 h-4 text-[#45d1a9] shrink-0" />
              <span className="font-mono truncate">
                Exported {lastExport.filename} ({formatBytes(lastExport.receipt.bytes)}) via{' '}
                {lastExport.receipt.engine} · SHA {lastExport.receipt.sha256.slice(0, 10)}
              </span>
            </div>
            <div className="flex items-center gap-3 shrink-0">
              {lastExport.receipt.fallback_used && lastExport.receipt.fallback_note && (
                <span className="text-[10.5px] text-[#f5bf5b]">
                  Fallback: {lastExport.receipt.fallback_note}
                </span>
              )}
              <a
                href={lastExport.downloadUrl}
                download={lastExport.filename}
                className="px-2.5 py-0.5 bg-[#1e4d3f] hover:bg-[#276452] border border-[#3a8d74] text-[#e7fff6] font-medium text-[11px]"
              >
                Download File
              </a>
            </div>
          </div>
        )}

        {error && (
          <div className="px-4 py-2.5 bg-[#2d1619] border-b border-[#7a2e38] text-[#ffb4be] text-[11.5px] font-mono shrink-0">
            {error}
          </div>
        )}

        {/* Main Scrollable Dark-Mode Content */}
        <div className="flex-1 overflow-y-auto p-4 space-y-5">
          {busy && !report && (
            <div className="py-16 flex flex-col items-center justify-center gap-2 text-[#98a6bc]">
              <Activity className="w-6 h-6 text-[#4bc3f2] animate-pulse" />
              <p className="text-[12px] font-mono">
                Running independent blind Before &amp; After Rust diagnostics…
              </p>
            </div>
          )}

          {report && activeTab !== 'markdown' && (
            <>
              {/* 1. Comparison Summary (Significant Numeric Deltas Only) */}
              {activeTab === 'whitepaper' && (
                <section className="bg-[#151820] border border-[#2a3140]">
                  <div className="px-3.5 py-2.5 bg-[#1b202b] border-b border-[#2a3140] flex items-center justify-between">
                    <div>
                      <h3 className="text-[12px] font-semibold text-[#f1f6fe]">
                        1. Comparison Summary (Significant Numeric Deltas)
                      </h3>
                      <p className="text-[10.5px] text-[#8d99ad]">
                        Computed strictly after both blind analyses finished; reports measurable shifts only.
                      </p>
                    </div>
                    <span className="font-mono text-[10px] text-[#5fe0b7]">
                      {report.comparison.deltas.length} numeric delta(s)
                    </span>
                  </div>
                  <div className="overflow-x-auto">
                    <table className="w-full text-left border-collapse text-[11px]">
                      <thead>
                        <tr className="bg-[#12161e] border-b border-[#252d3c] text-[#8b98ae] font-mono text-[10px]">
                          <th className="py-1.5 px-3">Metric</th>
                          <th className="py-1.5 px-3">Before</th>
                          <th className="py-1.5 px-3">After</th>
                          <th className="py-1.5 px-3">Measured Delta</th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-[#202634] font-mono">
                        {report.comparison.deltas.map((d, i) => (
                          <tr key={`${d.metric}-${i}`} className="hover:bg-[#191f2a]">
                            <td className="py-1.5 px-3 text-[#d5dff0] font-sans font-medium">
                              {d.metric}
                            </td>
                            <td className="py-1.5 px-3 text-[#9eb0cc]">{d.before_value}</td>
                            <td className="py-1.5 px-3 text-[#c8f5e6]">{d.after_value}</td>
                            <td className="py-1.5 px-3 text-[#6cd4f8]">{d.summary}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </section>
              )}

              {/* 2. High-Contrast Audio Buffer Plots (Same Scales) */}
              <section className="space-y-3">
                <div className="flex items-center justify-between">
                  <h3 className="text-[12px] font-semibold text-[#f1f6fe]">
                    {activeTab === 'whitepaper'
                      ? '2. Audio Buffer Diagnostic Plots (Shared Scales)'
                      : 'Audio Buffer Diagnostic Plots (Shared Scales)'}
                  </h3>
                  <span className="font-mono text-[10px] text-[#8d99ad]">
                    Generated directly from raw &amp; exported PCM buffers
                  </span>
                </div>

                <WaveformComparisonCanvas plots={report.plots} />

                <SpectrogramCanvas
                  title="SPECTROGRAM — ORIGINAL INPUT"
                  grid={report.plots.spectrogram_before_db}
                  cols={report.plots.spectrogram_cols}
                  rows={report.plots.spectrogram_rows}
                  durationS={report.plots.duration_s}
                  maxHz={report.plots.max_hz}
                  freqHz={report.plots.spectrogram_freq_hz}
                  mode="magnitude"
                  dbRange={report.plots.spectrogram_db_range}
                  diffMaxAbs={report.plots.diff_db_range[1]}
                  testId="report-spec-before-canvas"
                />

                <SpectrogramCanvas
                  title="SPECTROGRAM — FINAL EXPORT (IDENTICAL SCALE)"
                  grid={report.plots.spectrogram_after_db}
                  cols={report.plots.spectrogram_cols}
                  rows={report.plots.spectrogram_rows}
                  durationS={report.plots.duration_s}
                  maxHz={report.plots.max_hz}
                  freqHz={report.plots.spectrogram_freq_hz}
                  mode="magnitude"
                  dbRange={report.plots.spectrogram_db_range}
                  diffMaxAbs={report.plots.diff_db_range[1]}
                  testId="report-spec-after-canvas"
                />

                <SpectrogramCanvas
                  title="DIFFERENCE SPECTROGRAM (FINAL − ORIGINAL)"
                  grid={report.plots.spectrogram_diff_db}
                  cols={report.plots.spectrogram_cols}
                  rows={report.plots.spectrogram_rows}
                  durationS={report.plots.duration_s}
                  maxHz={report.plots.max_hz}
                  freqHz={report.plots.spectrogram_freq_hz}
                  mode="diff"
                  dbRange={report.plots.spectrogram_db_range}
                  diffMaxAbs={report.plots.diff_db_range[1]}
                  testId="report-spec-diff-canvas"
                />

                <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
                  <ThirdOctaveCanvas plots={report.plots} />
                  <UpperBandCanvas plots={report.plots} />
                </div>
              </section>

              {/* 3. Side-by-Side Blind Before & After Reports */}
              {activeTab === 'whitepaper' && (
                <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                  <FindingsTable
                    title="3. Before Analysis (Blind)"
                    subtitle="Raw input only — zero access to final export"
                    scopeBadge={report.before.blind_scope}
                    findings={report.before.findings}
                  />
                  <FindingsTable
                    title="4. After Analysis (Blind)"
                    subtitle="Final export only — zero access to Before report"
                    scopeBadge={report.after.blind_scope}
                    findings={report.after.findings}
                  />
                </div>
              )}

              {/* 4. Craft-Libs Integration & Fallbacks Note */}
              {activeTab === 'whitepaper' && (
                <section className="bg-[#131720] border border-[#262e3d] px-3.5 py-2.5 text-[10.5px] font-mono text-[#8e9bb2] space-y-1">
                  <div className="text-[#c8d5ea] font-semibold">
                    Export Engine &amp; craft-libs Integration
                  </div>
                  <div>
                    <span className="text-[#5fe0b7]">Used:</span>{' '}
                    {report.craft_libs.crates_used.join(' · ')}
                  </div>
                  <div>
                    <span className="text-[#f5bf5b]">Fallbacks:</span>{' '}
                    {report.craft_libs.unavailable_fallbacks.join(' ')}
                  </div>
                </section>
              )}
            </>
          )}

          {report && activeTab === 'markdown' && (
            <pre className="bg-[#12161f] border border-[#283142] p-4 font-mono text-[11px] leading-relaxed text-[#dce5f4] whitespace-pre-wrap overflow-x-auto select-text">
              {report.whitepaper_markdown}
            </pre>
          )}
        </div>
      </div>
    </div>
  );
}
