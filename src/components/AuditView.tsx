import React, { useEffect, useState } from 'react';
import { RefreshCw } from 'lucide-react';
import { WhitePaperSection } from './WhitePaperSection';

interface AuditData {
  ranAt: string;
  verifySource: { passed: boolean; output: string };
  auditMigration: { passed: boolean; replacedEntries: number; outputSummary: string };
  linuxPreflight: {
    passed: boolean;
    platform: string;
    audioQuality: string;
    mobileBuildsVerified: boolean;
  };
  rustBinaries: { cli: boolean; qa: boolean; cliSizeMb: number; qaSizeMb: number };
}

const PLATFORM_MATRIX = [
  { check: 'Linux x86_64 core + bench + guitar_controls (55 tests)', status: 'Passed', note: 'Verified via cargo test --locked --release --features bench --lib --tests' },
  { check: 'Linux x86_64 desktop app unit tests (5 tests)', status: 'Passed', note: 'Verified with local libasound.so.2 pkg-config metadata; no /dev/snd device' },
  { check: 'Rust formatting (cargo fmt --all --check)', status: 'Passed', note: 'Clean exit 0' },
  { check: 'Source fingerprints & migration audit (38/38 retired entries)', status: 'Passed', note: 'Verified live via spectracraft-qa verify-source and audit-migration' },
  { check: 'Physical audio output (/dev/snd) & human listening review', status: 'Not Run', note: 'Headless container; browser audition requires a human listener' },
  { check: 'Windows build, runtime & audio playback', status: 'Not Run', note: 'Linux host only; Windows must be verified independently' },
  { check: 'macOS build, runtime & audio playback', status: 'Not Run', note: 'Linux host only; macOS must be verified independently' },
  { check: 'iOS & Android builds / mobile playback', status: 'Not Run', note: 'Not built or tested on mobile targets' },
  { check: 'Dependency advisory scan (cargo audit)', status: 'Not Run', note: 'cargo-audit not installed; preflight executed with skip' },
];

export function AuditView() {
  const [viewMode, setViewMode] = useState<'audit' | 'spec'>('audit');
  const [audit, setAudit] = useState<AuditData | null>(null);
  const [loading, setLoading] = useState<boolean>(true);
  const [error, setError] = useState<string | null>(null);

  async function loadAudit(forceRun: boolean) {
    setLoading(true);
    setError(null);
    try {
      const resp = await fetch(forceRun ? '/api/audit/run' : '/api/audit', {
        method: forceRun ? 'POST' : 'GET',
      });
      const data = await resp.json();
      if (!resp.ok) {
        throw new Error(data.error || 'Failed to run Rust audit.');
      }
      setAudit(data as AuditData);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void loadAudit(false);
  }, []);

  return (
    <div className="space-y-6">
      <section className="flex flex-wrap items-start justify-between gap-4 border-b border-slate-200 pb-5">
        <div className="max-w-3xl space-y-1">
          <h1 className="text-2xl font-bold tracking-tight text-slate-900">
            {viewMode === 'audit'
              ? 'Rust Engine Audit & Platform Verification Matrix'
              : 'Technical White Paper (November 2026)'}
          </h1>
          <p className="text-sm text-slate-600">
            {viewMode === 'audit' ? (
              <>
                This browser workbench is a thin client over the native Rust binaries (
                <code className="font-mono text-xs">spectracraft-mono</code> and{' '}
                <code className="font-mono text-xs">spectracraft-qa</code>). The native Rust desktop
                app (<code className="font-mono text-xs">spectracraft-desktop</code>) and CLI
                tools are preserved intact with zero Python dependencies.
              </>
            ) : (
              <>
                System design, architectural boundaries, measured Slow Solo evidence, scope limits,
                and literature references — also stored at{' '}
                <code className="font-mono text-xs">spectracraft-mono/WHITE-PAPER.md</code>.
              </>
            )}
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <div className="flex items-center gap-1 rounded-md bg-slate-200/70 p-1">
            <button
              type="button"
              onClick={() => setViewMode('audit')}
              className={`whitespace-nowrap rounded px-3 py-1.5 text-xs font-medium transition-colors ${
                viewMode === 'audit'
                  ? 'bg-white text-slate-900 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              Rust Audit &amp; Matrix
            </button>
            <button
              type="button"
              onClick={() => setViewMode('spec')}
              className={`whitespace-nowrap rounded px-3 py-1.5 text-xs font-medium transition-colors ${
                viewMode === 'spec'
                  ? 'bg-white text-slate-900 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              Technical White Paper
            </button>
          </div>

          {viewMode === 'audit' && (
            <button
              type="button"
              disabled={loading}
              onClick={() => void loadAudit(true)}
              className="inline-flex items-center gap-1.5 whitespace-nowrap rounded-md bg-slate-900 px-4 py-2 text-sm font-medium text-white hover:bg-slate-800 disabled:opacity-60"
            >
              <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} aria-hidden="true" />
              {loading ? 'Running Rust Checks…' : 'Re-Run Rust Audit'}
            </button>
          )}
        </div>
      </section>

      {viewMode === 'spec' ? (
        <WhitePaperSection />
      ) : (
        <>
          {error && (
            <div role="alert" className="rounded-md border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800">
              {error}
            </div>
          )}

      {audit && (
        <section className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4">
          <div className="rounded-lg border border-slate-200 bg-white p-4">
            <p className="text-xs font-medium text-slate-500">Source SHA-256 Inventory</p>
            <p className="mt-1 text-lg font-semibold text-slate-900">
              {audit.verifySource.passed ? 'Passed' : 'Failed'}
            </p>
            <p className="mt-1 text-xs text-slate-500">{audit.verifySource.output}</p>
          </div>

          <div className="rounded-lg border border-slate-200 bg-white p-4">
            <p className="text-xs font-medium text-slate-500">Zero-Python Migration Audit</p>
            <p className="mt-1 text-lg font-semibold text-slate-900">
              {audit.auditMigration.passed ? 'Passed' : 'Failed'}
            </p>
            <p className="mt-1 text-xs text-slate-500">{audit.auditMigration.outputSummary}</p>
          </div>

          <div className="rounded-lg border border-slate-200 bg-white p-4">
            <p className="text-xs font-medium text-slate-500">Linux Core Preflight</p>
            <p className="mt-1 text-lg font-semibold text-slate-900">
              {audit.linuxPreflight.passed ? 'Passed (Linux x86_64)' : 'Failed'}
            </p>
            <p className="mt-1 text-xs text-slate-500">
              Audio quality: {audit.linuxPreflight.audioQuality} · Mobile verified:{' '}
              {audit.linuxPreflight.mobileBuildsVerified ? 'Yes' : 'No'}
            </p>
          </div>

          <div className="rounded-lg border border-slate-200 bg-white p-4">
            <p className="text-xs font-medium text-slate-500">Compiled Release Binaries</p>
            <p className="mt-1 font-mono text-lg font-semibold tabular-nums text-slate-900">
              {audit.rustBinaries.cliSizeMb} MB / {audit.rustBinaries.qaSizeMb} MB
            </p>
            <p className="mt-1 text-xs text-slate-500">
              spectracraft-mono &amp; spectracraft-qa (locked Cargo.lock)
            </p>
          </div>
        </section>
      )}

      <section className="rounded-lg border border-slate-200 bg-white p-5">
        <h2 className="text-base font-semibold text-slate-900">
          Honest Verification Matrix (Passed vs. Not Run)
        </h2>
        <p className="mt-1 text-xs text-slate-500">
          A Linux build and software test pass does not verify Windows, macOS, mobile builds,
          hardware playback, or perceived listening quality.
        </p>

        <div className="mt-4 overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-slate-200 text-xs font-medium text-slate-500">
                <th className="py-2 pr-4">Verification Item</th>
                <th className="py-2 px-4">Status</th>
                <th className="py-2 pl-4">Evidence / Scope</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100 text-xs">
              {PLATFORM_MATRIX.map((row) => (
                <tr key={row.check}>
                  <td className="py-2.5 pr-4 font-medium text-slate-900">{row.check}</td>
                  <td
                    className={`py-2.5 px-4 font-mono font-semibold ${
                      row.status === 'Passed' ? 'text-emerald-700' : 'text-amber-700'
                    }`}
                  >
                    {row.status}
                  </td>
                  <td className="py-2.5 pl-4 text-slate-600">{row.note}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section className="rounded-lg border border-slate-200 bg-white p-5">
        <h2 className="text-base font-semibold text-slate-900">
          Native Rust CLI &amp; Desktop Commands
        </h2>
        <pre className="mt-3 overflow-x-auto rounded-md border border-slate-200 bg-slate-50 p-4 font-mono text-xs leading-relaxed text-slate-800">{`cd spectracraft-mono
cargo fmt --all --check
cargo test --locked --release --features bench --lib --tests
cargo test --locked --features desktop --bin spectracraft-desktop
cargo run --locked --features desktop --bin spectracraft-desktop
target/release/spectracraft-mono repair-gap /tmp/slow-solo-ref/slow_solo_44100_mono.wav /tmp/out-repaired.wav 948150 948238
target/release/spectracraft-qa guitar-verify /tmp/slow-solo-ref/slow_solo_44100_mono.wav /tmp/out-verify
target/release/spectracraft-qa guitar-variants /tmp/slow-solo-ref/slow_solo_44100_mono.wav /tmp/out-verify/cues.json /tmp/out-variants all`}</pre>
      </section>
        </>
      )}
    </div>
  );
}
