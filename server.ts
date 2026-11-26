import crypto from 'node:crypto';
import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import express from 'express';
import type { Request, Response } from 'express';
import { createServer as createViteServer } from 'vite';
import {
  ALL_VARIANTS,
  countUncertainOverlaps,
  sanitizeClientErrorMessage,
  sliceMonoWavBuffer,
  validateGapRepairRange,
  validateMonoWavHeader,
  type FrameEvidence,
  type GapRepairResult,
  type IndependentPitchReport,
  type LayerSessionResult,
  type NoteEditPatch,
  type NoteSummary,
  type SessionPayload,
  type Strike,
  type VariantKey,
  type VariantsManifest,
  type VerificationReport,
} from './src/lib/workbench.ts';
import { CatalogLoadError, loadLocalStudioCatalogs } from './src/lib/localCatalogLoader.ts';

const execFileAsync = promisify(execFile);

const ROOT = process.cwd();
const CRATE_DIR = path.resolve(ROOT, 'spectracraft-mono');
const CAPTURE_REFERENCE_DIR = path.resolve(CRATE_DIR, 'capture-reference');

const BIN_EXT = process.platform === 'win32' ? '.exe' : '';

function resolveRustBinary(name: string): string {
  const filename = `${name}${BIN_EXT}`;
  const workspaceBin = path.resolve(ROOT, 'target/release', filename);
  if (fs.existsSync(workspaceBin)) return workspaceBin;
  return path.resolve(CRATE_DIR, 'target/release', filename);
}

const SC_BIN = resolveRustBinary('spectracraft-mono');
const SC_QA = resolveRustBinary('spectracraft-qa');

const TMP_ROOT = os.tmpdir();
const REF_WAV =
  process.env.SPECTRACRAFT_REF_WAV ??
  path.join(TMP_ROOT, 'slow-solo-ref', 'slow_solo_48000_mono_exact.wav');
const DEFAULT_VERIFY_DIR =
  process.env.SPECTRACRAFT_VERIFY_DIR ?? path.join(TMP_ROOT, 'slow-solo-verify-48k-exact');
const DEFAULT_VARIANTS_DIR =
  process.env.SPECTRACRAFT_VARIANTS_DIR ?? path.join(TMP_ROOT, 'slow-solo-variants-48k-exact');
const SESSIONS_ROOT =
  process.env.SPECTRACRAFT_SESSIONS_DIR ?? path.join(TMP_ROOT, 'spectracraft-sessions');
const EMPTY_RETIRED_DIR = path.join(TMP_ROOT, 'spectracraft-empty-retired');

function ensurePrivateDir(dir: string): void {
  fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
  if (process.platform !== 'win32') {
    try {
      fs.chmodSync(dir, 0o700);
    } catch {
      // ignore chmod failures on restricted filesystems
    }
  }
}

const TRANSIENT_WORK_SUBDIRS = [
  'cue-render-',
  'source-regen-',
  'region-edit-',
  'layers-',
  'gap-repair-',
  'variants-',
];
const SHARED_WORK_SUBDIRS = new Set(['pipeline', 'captures', 'reports', 'daw-interchange']);

function pruneUnreferencedSessionDirs(): void {
  try {
    for (const entry of fs.readdirSync(SESSIONS_ROOT, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const fullPath = path.join(SESSIONS_ROOT, entry.name);
      if (!isConfinedPath(SESSIONS_ROOT, fullPath)) continue;
      // On startup the in-memory sessions Map is empty, so all prior upload-* and scratch dirs are unreferenced.
      if (
        entry.name.startsWith('upload-') ||
        entry.name === 'analysis-tmp' ||
        entry.name === 'settings-tmp'
      ) {
        try {
          fs.rmSync(fullPath, { recursive: true, force: true });
        } catch {
          // ignore individual cleanup errors
        }
      } else if (entry.name === 'slow-solo-work') {
        try {
          for (const sub of fs.readdirSync(fullPath, { withFileTypes: true })) {
            const subPath = path.join(fullPath, sub.name);
            if (!isConfinedPath(fullPath, subPath)) continue;
            if (
              SHARED_WORK_SUBDIRS.has(sub.name) ||
              TRANSIENT_WORK_SUBDIRS.some((prefix) => sub.name.startsWith(prefix))
            ) {
              fs.rmSync(subPath, { recursive: true, force: true });
            }
          }
        } catch {
          // ignore slow-solo-work cleanup errors
        }
      }
    }
  } catch {
    // ignore startup prune errors
  }
}

ensurePrivateDir(SESSIONS_ROOT);
ensurePrivateDir(EMPTY_RETIRED_DIR);
pruneUnreferencedSessionDirs();

const HOME_DIR = os.homedir();
const HOME_CARGO_BIN = HOME_DIR ? path.join(HOME_DIR, '.cargo', 'bin') : '';
const DEFAULT_CARGO_HOME =
  HOME_DIR && fs.existsSync(path.join(HOME_DIR, '.cargo'))
    ? path.join(HOME_DIR, '.cargo')
    : path.join(TMP_ROOT, 'cargo');
const DEFAULT_RUSTUP_HOME =
  HOME_DIR && fs.existsSync(path.join(HOME_DIR, '.rustup'))
    ? path.join(HOME_DIR, '.rustup')
    : path.join(TMP_ROOT, 'rustup');

const RUST_ENV: NodeJS.ProcessEnv = {
  ...process.env,
  PATH: [HOME_CARGO_BIN, path.join(DEFAULT_CARGO_HOME, 'bin'), process.env.PATH ?? '']
    .filter(Boolean)
    .join(path.delimiter),
  RUSTUP_HOME: process.env.RUSTUP_HOME ?? DEFAULT_RUSTUP_HOME,
  CARGO_HOME: process.env.CARGO_HOME ?? DEFAULT_CARGO_HOME,
};

function sanitizeDownloadName(raw: string): string {
  const base = path.basename(raw.replace(/\\/g, '/'));
  const cleaned = base.replace(/[\r\n"\\/;:*?<>|\x00-\x1f\x7f]/g, '_').trim();
  return cleaned.slice(0, 120) || 'artifact';
}

function isConfinedPath(baseDir: string, candidatePath: string): boolean {
  const resolvedBase = path.resolve(baseDir);
  const resolvedCandidate = path.resolve(candidatePath);
  return (
    resolvedCandidate === resolvedBase ||
    resolvedCandidate.startsWith(resolvedBase + path.sep)
  );
}

interface SessionRecord {
  id: string;
  label: string;
  isReference: boolean;
  attribution: string | null;
  sourceWav: string;
  verifyDir: string;
  workDir: string;
  variantsDir: string | null;
  editedCues: Map<string, string>;
  editedDryWavs: Map<string, string>;
  pipelineOutputs: Map<string, string>;
  lastOutputWav: string | null;
  lastOutputLabel: string | null;
  reports: Map<string, string>;
  auxRuns: Map<string, string>;
}

const MAX_UPLOADED_SESSIONS = 16;
const MAX_RUN_ENTRIES_PER_SESSION = 24;
const MAX_MEDIA_ENTRIES = 512;
const SESSION_ID_RE = /^[a-zA-Z0-9_-]{1,64}$/;

const sessions = new Map<string, SessionRecord>();
const mediaRegistry = new Map<string, { filePath: string; contentType: string; downloadName: string }>();
let cachedStudioCatalogs: unknown | null = null;

function sanitizeClientError(err: unknown, fallback = 'Request failed.'): string {
  const rawStderr =
    typeof err === 'object' && err !== null && 'stderr' in err
      ? String((err as { stderr?: unknown }).stderr ?? '').trim()
      : '';
  const raw = rawStderr || (err instanceof Error ? err.message : String(err ?? ''));
  return sanitizeClientErrorMessage(raw, [ROOT, SESSIONS_ROOT, TMP_ROOT, HOME_DIR], fallback);
}

function pruneMediaUnder(targetPath: string): void {
  for (const [key, entry] of mediaRegistry.entries()) {
    if (isConfinedPath(targetPath, entry.filePath)) {
      mediaRegistry.delete(key);
    }
  }
}

function resolveEvictionTarget(rec: SessionRecord, candidatePath: string): string | null {
  const abs = path.resolve(candidatePath);
  if (!isConfinedPath(SESSIONS_ROOT, abs) || !isConfinedPath(rec.workDir, abs)) {
    return null;
  }
  if (isConfinedPath(rec.verifyDir, abs) || abs === path.resolve(rec.workDir)) {
    return null;
  }
  const rel = path.relative(rec.workDir, abs);
  const parts = rel.split(path.sep).filter(Boolean);
  if (parts.length === 0) return null;
  const top = parts[0];
  if (TRANSIENT_WORK_SUBDIRS.some((prefix) => top.startsWith(prefix))) {
    return path.join(rec.workDir, top);
  }
  if (top === 'daw-interchange' && parts.length >= 2) {
    return path.join(rec.workDir, top, parts[1]);
  }
  return abs;
}

function isPathStillReferenced(rec: SessionRecord, targetPath: string): boolean {
  if (rec.lastOutputWav && isConfinedPath(targetPath, rec.lastOutputWav)) return true;
  if (rec.variantsDir && isConfinedPath(targetPath, rec.variantsDir)) return true;
  for (const map of [rec.editedCues, rec.editedDryWavs, rec.pipelineOutputs, rec.reports, rec.auxRuns]) {
    for (const p of map.values()) {
      if (isConfinedPath(targetPath, p)) return true;
    }
  }
  return false;
}

function cleanupEvictedRunPath(rec: SessionRecord, evictedKey: string, evictedPath: string): void {
  const target = resolveEvictionTarget(rec, evictedPath);
  if (!target || isPathStillReferenced(rec, target)) return;
  pruneMediaUnder(target);
  if (fs.existsSync(target)) {
    try {
      fs.rmSync(target, { recursive: true, force: true });
    } catch {
      // ignore run eviction cleanup failure
    }
  }
  // When a technical report JSON is evicted, also remove any exported PDF/MD files for that reportId.
  const reportsDir = path.join(rec.workDir, 'reports');
  if (isConfinedPath(reportsDir, target) && fs.existsSync(reportsDir)) {
    const prefix = `spectracraft-report-${evictedKey}-`;
    try {
      for (const name of fs.readdirSync(reportsDir)) {
        if (!name.startsWith(prefix)) continue;
        const exported = path.join(reportsDir, name);
        if (isConfinedPath(reportsDir, exported)) {
          pruneMediaUnder(exported);
          fs.rmSync(exported, { force: true });
        }
      }
    } catch {
      // ignore report export cleanup failure
    }
  }
}

function setBoundedSessionMap(
  rec: SessionRecord,
  map: Map<string, string>,
  key: string,
  value: string,
): void {
  if (map.has(key)) {
    map.delete(key);
  }
  map.set(key, value);
  while (map.size > MAX_RUN_ENTRIES_PER_SESSION) {
    const oldestKey = map.keys().next().value;
    if (oldestKey === undefined) break;
    const oldestPath = map.get(oldestKey);
    map.delete(oldestKey);
    if (oldestPath) {
      cleanupEvictedRunPath(rec, oldestKey, oldestPath);
    }
  }
}

function evictOldUploadedSessions(): void {
  const uploaded = [...sessions.values()].filter((s) => !s.isReference);
  while (uploaded.length > MAX_UPLOADED_SESSIONS) {
    const oldest = uploaded.shift();
    if (!oldest) break;
    sessions.delete(oldest.id);
    pruneMediaUnder(oldest.workDir);
    if (isConfinedPath(SESSIONS_ROOT, oldest.workDir) && fs.existsSync(oldest.workDir)) {
      try {
        fs.rmSync(oldest.workDir, { recursive: true, force: true });
      } catch {
        // ignore eviction cleanup error
      }
    }
  }
}

function registerMedia(filePath: string, contentType: string, downloadName?: string): string {
  const abs = path.resolve(filePath);
  const key = crypto.createHash('sha256').update(abs).digest('hex').slice(0, 24);
  if (mediaRegistry.has(key)) {
    mediaRegistry.delete(key);
  }
  mediaRegistry.set(key, {
    filePath: abs,
    contentType,
    downloadName: sanitizeDownloadName(downloadName ?? path.basename(abs)),
  });
  while (mediaRegistry.size > MAX_MEDIA_ENTRIES) {
    const oldestKey = mediaRegistry.keys().next().value;
    if (oldestKey === undefined) break;
    mediaRegistry.delete(oldestKey);
  }
  return `/api/media/${key}`;
}

const MAX_CONCURRENT_RUST_JOBS = 4;
const MAX_QUEUED_RUST_JOBS = 12;
let activeRustJobs = 0;
const rustJobQueue: Array<() => void> = [];

async function acquireRustSlot(): Promise<void> {
  if (activeRustJobs < MAX_CONCURRENT_RUST_JOBS) {
    activeRustJobs += 1;
    return;
  }
  if (rustJobQueue.length >= MAX_QUEUED_RUST_JOBS) {
    throw new Error('Server is busy processing audio jobs. Please retry in a moment.');
  }
  await new Promise<void>((resolve) => {
    rustJobQueue.push(resolve);
  });
  activeRustJobs += 1;
}

function releaseRustSlot(): void {
  activeRustJobs = Math.max(0, activeRustJobs - 1);
  const next = rustJobQueue.shift();
  if (next) next();
}

async function runRust(bin: string, args: string[], timeoutMs = 180_000): Promise<{ stdout: string; stderr: string }> {
  if (!fs.existsSync(bin)) {
    throw new Error(`Compiled Rust binary "${path.basename(bin)}" is not available.`);
  }
  await acquireRustSlot();
  try {
    const { stdout, stderr } = await execFileAsync(bin, args, {
      cwd: ROOT,
      env: RUST_ENV,
      timeout: timeoutMs,
      maxBuffer: 32 * 1024 * 1024,
    });
    return { stdout, stderr };
  } finally {
    releaseRustSlot();
  }
}

interface RawCueNote {
  start: number;
  end: number;
  hz: number[];
  level_steps: number[];
  re_picks?: Strike[] | null;
  shape?: Record<string, unknown>;
  accent?: number | null;
}

interface RawCuePerformance {
  rate: number;
  samples: number;
  hop_s: number;
  notes: RawCueNote[];
  frame_evidence?: FrameEvidence[];
}

function summarizeNotes(raw: RawCuePerformance): NoteSummary[] {
  const frames = raw.frame_evidence ?? [];
  return raw.notes.map((n, index) => {
    const sorted = [...n.hz].sort((a, b) => a - b);
    const mid = sorted.length > 0 ? sorted[Math.floor(sorted.length / 2)] : 0;
    const minHz = sorted[0] ?? 0;
    const maxHz = sorted[sorted.length - 1] ?? 0;
    let peak = 0;
    for (const v of n.level_steps) {
      if (v > peak) peak = v;
    }
    return {
      index,
      start: n.start,
      end: n.end,
      duration_s: Math.max(0, n.end - n.start),
      median_hz: mid,
      min_hz: minHz,
      max_hz: maxHz,
      peak_level: peak,
      re_picks: Array.isArray(n.re_picks) ? n.re_picks : [],
      frames: n.hz.length,
      uncertain_overlap_count: countUncertainOverlaps(n.start, n.end, frames),
    };
  });
}

function extractTransitionEvidence(raw: RawCuePerformance): FrameEvidence[] {
  const frames = raw.frame_evidence ?? [];
  const inWindow = frames.filter((f) => f.time_s >= 25.0 && f.time_s <= 26.0);
  if (inWindow.length > 0) {
    return inWindow;
  }
  // For shorter uploaded clips, return up to 120 frames focusing on uncertain/out_of_target frames first.
  const interesting = frames.filter((f) => f.status === 'uncertain' || f.status === 'out_of_target');
  return (interesting.length > 0 ? interesting : frames).slice(0, 120);
}

function hydrateVariants(variantsDir: string): VariantsManifest | null {
  const manifestPath = path.join(variantsDir, 'variants-manifest.json');
  if (!fs.existsSync(manifestPath)) {
    return null;
  }
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as VariantsManifest;
  manifest.exports = manifest.exports.map((entry) => {
    const wavPath = path.resolve(variantsDir, String(entry.file ?? ''));
    const safe = isConfinedPath(variantsDir, wavPath) && fs.existsSync(wavPath);
    return {
      ...entry,
      url: safe ? registerMedia(wavPath, 'audio/wav', sanitizeDownloadName(entry.file)) : undefined,
    };
  });
  const wfFull = path.join(variantsDir, 'waveforms-full.svg');
  const wfDetail = path.join(variantsDir, 'waveforms-detail.svg');
  const spFull = path.join(variantsDir, 'spectrograms-full.svg');
  const spDetail = path.join(variantsDir, 'spectrograms-detail.svg');
  if (fs.existsSync(wfFull) && fs.existsSync(wfDetail) && fs.existsSync(spFull) && fs.existsSync(spDetail)) {
    manifest.figures = {
      waveformsFull: registerMedia(wfFull, 'image/svg+xml', 'waveforms-full.svg'),
      waveformsDetail: registerMedia(wfDetail, 'image/svg+xml', 'waveforms-detail.svg'),
      spectrogramsFull: registerMedia(spFull, 'image/svg+xml', 'spectrograms-full.svg'),
      spectrogramsDetail: registerMedia(spDetail, 'image/svg+xml', 'spectrograms-detail.svg'),
    };
  }
  const reportPath = path.join(variantsDir, 'technical-report.md');
  if (fs.existsSync(reportPath)) {
    manifest.technicalReport = fs.readFileSync(reportPath, 'utf8');
  }
  return manifest;
}

function buildSessionPayload(rec: SessionRecord): SessionPayload {
  const verifyJsonPath = path.join(rec.verifyDir, 'verification.json');
  const reportPath = path.join(rec.verifyDir, 'report.md');
  const cuesPath = path.join(rec.verifyDir, 'cues.json');
  const dryPath = path.join(rec.verifyDir, 'dry-candidate.wav');
  const abPath = path.join(rec.verifyDir, 'ab-original-vs-dry-candidate.wav');
  const wfFull = path.join(rec.verifyDir, 'waveforms-full.svg');
  const wfDetail = path.join(rec.verifyDir, 'waveforms-20-27s.svg');
  const spFull = path.join(rec.verifyDir, 'spectrograms-full.svg');
  const spDetail = path.join(rec.verifyDir, 'spectrograms-20-27s.svg');
  const spTrans = path.join(rec.verifyDir, 'spectrograms-25-26s.svg');

  const verification = JSON.parse(fs.readFileSync(verifyJsonPath, 'utf8')) as VerificationReport;
  const reportMarkdown = fs.existsSync(reportPath) ? fs.readFileSync(reportPath, 'utf8') : '';
  const rawCues = JSON.parse(fs.readFileSync(cuesPath, 'utf8')) as RawCuePerformance;

  return {
    id: rec.id,
    label: rec.label,
    isReference: rec.isReference,
    attribution: rec.attribution,
    verification,
    reportMarkdown,
    files: {
      sourceWav: registerMedia(rec.sourceWav, 'audio/wav', `${rec.id}-original.wav`),
      dryWav: registerMedia(dryPath, 'audio/wav', `${rec.id}-dry-candidate.wav`),
      abWav: registerMedia(abPath, 'audio/wav', `${rec.id}-ab-level-matched.wav`),
      cuesJson: registerMedia(cuesPath, 'application/json', `${rec.id}-cues.json`),
      waveformsFull: registerMedia(wfFull, 'image/svg+xml', 'waveforms-full.svg'),
      waveformsDetail: fs.existsSync(wfDetail)
        ? registerMedia(wfDetail, 'image/svg+xml', 'waveforms-20-27s.svg')
        : null,
      spectrogramsFull: registerMedia(spFull, 'image/svg+xml', 'spectrograms-full.svg'),
      spectrogramsDetail: fs.existsSync(spDetail)
        ? registerMedia(spDetail, 'image/svg+xml', 'spectrograms-20-27s.svg')
        : null,
      spectrogramsTransition: fs.existsSync(spTrans)
        ? registerMedia(spTrans, 'image/svg+xml', 'spectrograms-25-26s.svg')
        : null,
    },
    notes: summarizeNotes(rawCues),
    transitionEvidence: extractTransitionEvidence(rawCues),
    variants: rec.variantsDir ? hydrateVariants(rec.variantsDir) : null,
  };
}

let defaultSessionPromise: Promise<SessionRecord> | null = null;

async function ensureDefaultSession(): Promise<SessionRecord> {
  const existing = sessions.get('slow-solo-ref');
  if (existing && fs.existsSync(path.join(existing.verifyDir, 'verification.json'))) {
    return existing;
  }
  if (defaultSessionPromise) {
    return defaultSessionPromise;
  }
  defaultSessionPromise = (async () => {
    let sourcePath = REF_WAV;
    let label = 'slow_solo_48000_mono_exact.wav';
    let attribution =
      'Wikimedia Commons File:Slow_solo_20110108.ogg · David Monniaux (2011-01-08) · CC BY-SA 3.0 / GFDL';
    let verifyDir = DEFAULT_VERIFY_DIR;

    if (!fs.existsSync(REF_WAV)) {
      // Fallback to deterministic real Rust 48 kHz mono fixture generated by spectracraft-qa six-note
      sourcePath = path.join(SESSIONS_ROOT, 'six-note-fixture-48k.wav');
      label = 'six-note-fixture-48k.wav';
      attribution = 'Deterministic 48 kHz mono fixture generated by Rust spectracraft-qa six-note (reference WAV not present in /tmp).';
      verifyDir = path.join(SESSIONS_ROOT, 'six-note-verify');
      if (!fs.existsSync(sourcePath)) {
        await runRust(SC_QA, ['six-note', sourcePath]);
      }
    }

    const cachedVerifyJson = path.join(verifyDir, 'verification.json');
    let canReuseDefault = false;
    if (fs.existsSync(cachedVerifyJson)) {
      try {
        const parsed = JSON.parse(fs.readFileSync(cachedVerifyJson, 'utf8')) as { source_path?: string };
        canReuseDefault = parsed.source_path === sourcePath;
      } catch {
        canReuseDefault = false;
      }
    }

    if (!canReuseDefault) {
      if (fs.existsSync(verifyDir)) {
        fs.rmSync(verifyDir, { recursive: true, force: true });
      }
      await runRust(SC_QA, ['guitar-verify', sourcePath, verifyDir]);
    }

    const workDir = path.join(SESSIONS_ROOT, 'slow-solo-work');
    ensurePrivateDir(workDir);

    const record: SessionRecord = {
      id: 'slow-solo-ref',
      label,
      isReference: true,
      attribution,
      sourceWav: sourcePath,
      verifyDir,
      workDir,
      variantsDir: fs.existsSync(path.join(DEFAULT_VARIANTS_DIR, 'variants-manifest.json'))
        ? DEFAULT_VARIANTS_DIR
        : null,
      editedCues: new Map(),
      editedDryWavs: new Map(),
      pipelineOutputs: new Map(),
      lastOutputWav: null,
      lastOutputLabel: null,
      reports: new Map(),
      auxRuns: new Map(),
    };
    sessions.set(record.id, record);
    return record;
  })().finally(() => {
    defaultSessionPromise = null;
  });
  return defaultSessionPromise;
}

function getSession(id: string): SessionRecord | null {
  if (!SESSION_ID_RE.test(id)) return null;
  return sessions.get(id) ?? null;
}

function parseSelectionStrict(raw: string): string {
  const cleaned = raw.trim().toLowerCase();
  if (cleaned === 'all') return 'all';
  const parts = cleaned
    .split(',')
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.length === 0) {
    throw new Error('Select at least one variant (or "all").');
  }
  const valid = new Set<string>(ALL_VARIANTS);
  const unique: VariantKey[] = [];
  for (const part of parts) {
    if (!valid.has(part)) {
      throw new Error(`Unknown export variant: ${part}`);
    }
    const key = part as VariantKey;
    if (!unique.includes(key)) unique.push(key);
  }
  return unique.length === ALL_VARIANTS.length ? 'all' : unique.join(',');
}

interface AuditResult {
  ranAt: string;
  verifySource: { passed: boolean; output: string };
  auditMigration: { passed: boolean; replacedEntries: number; outputSummary: string };
  linuxPreflight: { passed: boolean; platform: string; audioQuality: string; mobileBuildsVerified: boolean };
  rustBinaries: { cli: boolean; qa: boolean; cliSizeMb: number; qaSizeMb: number };
}

let cachedAudit: AuditResult | null = null;

async function runRustAudit(): Promise<AuditResult> {
  const [srcRes, migRes, preRes] = await Promise.all([
    runRust(SC_QA, ['verify-source', CRATE_DIR, path.join(CRATE_DIR, 'SOURCE-SHA256.json')]),
    runRust(SC_QA, ['audit-migration', EMPTY_RETIRED_DIR, CRATE_DIR]),
    runRust(SC_QA, ['preflight', CRATE_DIR, 'linux', 'core', 'skip']),
  ]);

  const migJson = JSON.parse(migRes.stdout) as {
    active_source_files?: number;
    active_source_rust_only?: boolean;
    retired_script_count?: number;
    scripts?: Array<{ status?: string }>;
  };
  const preJson = JSON.parse(preRes.stdout) as {
    passed?: boolean;
    platform?: string;
    audio_quality?: string;
    manifest?: { mobile_builds_verified?: boolean };
  };

  const cliStat = fs.existsSync(SC_BIN) ? fs.statSync(SC_BIN) : null;
  const qaStat = fs.existsSync(SC_QA) ? fs.statSync(SC_QA) : null;
  const replacedCount = migJson.retired_script_count ?? migJson.scripts?.length ?? 0;
  const allReplaced =
    Boolean(migJson.active_source_rust_only) &&
    replacedCount > 0 &&
    (migJson.scripts?.every((s) => s.status === 'replaced') ?? false);

  const result: AuditResult = {
    ranAt: new Date().toISOString(),
    verifySource: {
      passed: srcRes.stdout.includes('All source fingerprints match'),
      output: srcRes.stdout.trim(),
    },
    auditMigration: {
      passed: allReplaced,
      replacedEntries: replacedCount,
      outputSummary: `${replacedCount} retired entries verified replaced (${migJson.active_source_files ?? 0} .rs sources only)`,
    },
    linuxPreflight: {
      passed: Boolean(preJson.passed),
      platform: preJson.platform ?? 'linux',
      audioQuality: preJson.audio_quality ?? 'not assessed',
      mobileBuildsVerified: Boolean(preJson.manifest?.mobile_builds_verified),
    },
    rustBinaries: {
      cli: Boolean(cliStat),
      qa: Boolean(qaStat),
      cliSizeMb: cliStat ? Number((cliStat.size / (1024 * 1024)).toFixed(2)) : 0,
      qaSizeMb: qaStat ? Number((qaStat.size / (1024 * 1024)).toFixed(2)) : 0,
    },
  };
  cachedAudit = result;
  return result;
}

async function startServer() {
  const app = express();
  app.disable('x-powered-by');
  app.use((_req, res, next) => {
    res.setHeader('X-Content-Type-Options', 'nosniff');
    res.setHeader('Referrer-Policy', 'no-referrer');
    next();
  });
  // Enforce same-origin on mutating API routes whenever an Origin header is supplied (no wildcard CORS).
  app.use('/api', (req, res, next) => {
    if (req.method === 'GET' || req.method === 'HEAD' || req.method === 'OPTIONS') {
      next();
      return;
    }
    const originHeader = req.headers.origin;
    if (typeof originHeader === 'string' && originHeader.length > 0) {
      try {
        const originHost = new URL(originHeader).host.toLowerCase();
        const forwardedHost = String(req.headers['x-forwarded-host'] ?? '')
          .split(',')[0]
          .trim()
          .toLowerCase();
        const reqHost = String(req.headers.host ?? '').trim().toLowerCase();
        const normalizeLoopback = (h: string) => h.replace(/^127\.0\.0\.1(?=:|$)/, 'localhost');
        const allowedHosts = new Set(
          [reqHost, forwardedHost].filter(Boolean).flatMap((h) => [h, normalizeLoopback(h)]),
        );
        if (!allowedHosts.has(originHost) && !allowedHosts.has(normalizeLoopback(originHost))) {
          res.status(403).json({ error: 'Cross-origin API mutation blocked.' });
          return;
        }
      } catch {
        res.status(403).json({ error: 'Malformed Origin header.' });
        return;
      }
    }
    next();
  });
  app.use(express.json({ limit: '4mb' }));

  app.get('/api/health', (_req: Request, res: Response) => {
    res.json({
      ok: true,
      engine: 'spectracraft-mono (Rust)',
      cliPresent: fs.existsSync(SC_BIN),
      qaPresent: fs.existsSync(SC_QA),
    });
  });

  app.get('/api/sessions/default', async (_req: Request, res: Response) => {
    try {
      const rec = await ensureDefaultSession();
      res.json(buildSessionPayload(rec));
    } catch (err) {
      res.status(500).json({ error: sanitizeClientError(err, 'Failed to initialize reference session.') });
    }
  });

  app.post(
    '/api/sessions/upload',
    express.raw({ type: ['audio/*', 'application/octet-stream'], limit: '45mb' }),
    async (req: Request, res: Response) => {
      let workDir: string | null = null;
      try {
        const body = req.body as Buffer;
        if (!Buffer.isBuffer(body)) {
          res.status(400).json({ error: 'Upload a valid mono RIFF/WAVE file (8–96 kHz, up to 90 seconds).' });
          return;
        }
        const wavCheck = validateMonoWavHeader(body);
        if (!wavCheck.ok) {
          res.status(400).json({ error: wavCheck.error });
          return;
        }
        const rawName = String(req.headers['x-filename'] ?? 'uploaded-audio.wav');
        const safeName = rawName.replace(/[^a-zA-Z0-9._ -]/g, '_').slice(0, 80) || 'uploaded-audio.wav';
        const sessionId = `upload-${crypto.randomUUID().slice(0, 10)}`;
        workDir = path.join(SESSIONS_ROOT, sessionId);
        ensurePrivateDir(workDir);
        const sourceWav = path.join(workDir, 'source.wav');
        fs.writeFileSync(sourceWav, body, { flag: 'wx', mode: 0o400 });
        const verifyDir = path.join(workDir, 'verify');

        await runRust(SC_QA, ['guitar-verify', sourceWav, verifyDir]);

        const rec: SessionRecord = {
          id: sessionId,
          label: safeName,
          isReference: false,
          attribution: null,
          sourceWav,
          verifyDir,
          workDir,
          variantsDir: null,
          editedCues: new Map(),
          editedDryWavs: new Map(),
          pipelineOutputs: new Map(),
          lastOutputWav: null,
          lastOutputLabel: null,
          reports: new Map(),
          auxRuns: new Map(),
        };
        sessions.set(sessionId, rec);
        evictOldUploadedSessions();
        workDir = null;
        res.json(buildSessionPayload(rec));
      } catch (err) {
        if (workDir && fs.existsSync(workDir)) {
          try {
            fs.rmSync(workDir, { recursive: true, force: true });
          } catch {
            // ignore cleanup failure
          }
        }
        res.status(400).json({
          error: sanitizeClientError(
            err,
            'Rust guitar-verify rejected this WAV (requires mono 8–96 kHz, <= 90 s).',
          ),
        });
      }
    },
  );

  app.post('/api/sessions/:id/render-cues', async (req: Request, res: Response) => {
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }
      const patches = (req.body?.patches ?? []) as NoteEditPatch[];
      if (!Array.isArray(patches) || patches.length > 2000) {
        res.status(400).json({ error: 'Expected patches array (max 2000 entries).' });
        return;
      }

      const baseCuesPath = path.join(rec.verifyDir, 'cues.json');
      const rawCues = JSON.parse(fs.readFileSync(baseCuesPath, 'utf8')) as RawCuePerformance;

      for (const patch of patches) {
        if (!patch || !Number.isInteger(patch.index) || patch.index < 0 || patch.index >= rawCues.notes.length) {
          continue;
        }
        const note = rawCues.notes[patch.index];
        if (!note) continue;
        const scale = Math.max(0, Math.min(2.0, Number.isFinite(patch.level_scale) ? patch.level_scale : 1));
        if (Math.abs(scale - 1.0) > 1e-6) {
          note.level_steps = note.level_steps.map((v) => Math.max(0, Math.min(3.85, v * scale)));
        }
        if (Array.isArray(patch.re_picks)) {
          const dur = Math.max(0, note.end - note.start);
          const cleanStrikes = patch.re_picks
            .slice(0, 64)
            .filter(
              (s) =>
                s &&
                Number.isFinite(s.offset_s) &&
                Number.isFinite(s.strength) &&
                s.offset_s >= 0 &&
                s.offset_s < dur &&
                s.strength >= 0 &&
                s.strength <= 1.5,
            )
            .sort((a, b) => a.offset_s - b.offset_s);
          note.re_picks = cleanStrikes.length > 0 ? cleanStrikes : null;
        }
      }

      const runId = crypto.randomUUID().slice(0, 8);
      const runDir = path.join(rec.workDir, `cue-render-${runId}`);
      ensurePrivateDir(runDir);
      const editedCuesPath = path.join(runDir, 'cues-edited.json');
      const dryWavPath = path.join(runDir, 'dry-edited-candidate.wav');
      fs.writeFileSync(editedCuesPath, JSON.stringify(rawCues), { flag: 'wx', mode: 0o600 });

      // Purely source-free Rust dry render from edited cue JSON alone.
      await runRust(SC_BIN, ['render-dry-guitar', editedCuesPath, dryWavPath]);

      const durationS = rawCues.samples / rawCues.rate;
      const [hashRes, pitchRes] = await Promise.all([
        runRust(SC_QA, ['hash', dryWavPath]),
        runRust(SC_QA, ['guitar-pitch-check', rec.sourceWav, dryWavPath, '0', String(durationS)]),
      ]);

      const pitchReport = JSON.parse(pitchRes.stdout) as IndependentPitchReport;
      const drySha256 = hashRes.stdout.trim().split(/\s+/)[0] ?? '';
      setBoundedSessionMap(rec, rec.editedCues, runId, editedCuesPath);
      setBoundedSessionMap(rec, rec.editedDryWavs, runId, dryWavPath);
      rec.lastOutputWav = dryWavPath;
      rec.lastOutputLabel = `Stage 1 Cue Dry Render (${runId})`;

      res.json({
        runId,
        drySha256,
        dryWavUrl: registerMedia(dryWavPath, 'audio/wav', `dry-edited-${runId}.wav`),
        cuesJsonUrl: registerMedia(editedCuesPath, 'application/json', `cues-edited-${runId}.json`),
        pitchCheck: pitchReport,
        notes: summarizeNotes(rawCues),
      });
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to render edited cues.') });
    }
  });

  app.get('/api/sessions/:id/reference-shape', async (req: Request, res: Response) => {
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }
      const sourceIndexRaw = req.query.sourceIndex;
      const sourceIndex =
        sourceIndexRaw !== undefined && Number.isFinite(Number(sourceIndexRaw))
          ? Math.max(0, Math.min(137, Math.floor(Number(sourceIndexRaw))))
          : null;
      const cuesRunId = typeof req.query.cuesRunId === 'string' ? req.query.cuesRunId : '';
      const cuesPath =
        (cuesRunId && rec.editedCues.get(cuesRunId)) || path.join(rec.verifyDir, 'cues.json');
      if (!fs.existsSync(rec.sourceWav) || !fs.existsSync(cuesPath)) {
        res.status(404).json({ error: 'Reference audio or extracted cues file not found.' });
        return;
      }
      const args = ['reference-shape', rec.sourceWav, cuesPath];
      if (sourceIndex !== null) args.push(String(sourceIndex));
      const { stdout } = await runRust(SC_QA, args);
      res.json(JSON.parse(stdout));
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to compute reference shape.') });
    }
  });

  app.post('/api/sessions/:id/regenerate-source', async (req: Request, res: Response) => {
    let voicingPath: string | null = null;
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }
      const sourceIndex = Math.max(0, Math.floor(Number(req.body?.sourceIndex ?? 1) || 0));
      const cuesRunId = typeof req.body?.cuesRunId === 'string' ? req.body.cuesRunId : '';
      const cuesPath =
        (cuesRunId && rec.editedCues.get(cuesRunId)) || path.join(rec.verifyDir, 'cues.json');
      if (!fs.existsSync(cuesPath)) {
        res.status(400).json({ error: 'Extracted performance cues JSON not found for session.' });
        return;
      }
      const rawCues = JSON.parse(fs.readFileSync(cuesPath, 'utf8')) as RawCuePerformance;

      const runId = crypto.randomUUID().slice(0, 8);
      const runDir = path.join(rec.workDir, `source-regen-${runId}`);
      ensurePrivateDir(runDir);
      const dryWavPath = path.join(runDir, `dry-source-${sourceIndex}.wav`);

      const args = ['render-dry-source', cuesPath, String(sourceIndex), dryWavPath];
      if (req.body?.voicing && typeof req.body.voicing === 'object') {
        voicingPath = path.join(runDir, `voicing-${runId}.json`);
        fs.writeFileSync(voicingPath, JSON.stringify(req.body.voicing), { encoding: 'utf8', mode: 0o600 });
        args.push(voicingPath);
      }

      // Purely cue-driven Rust DDSP source-informed dry regeneration (zero original audio blended in).
      const { stdout } = await runRust(SC_QA, args);
      const renderReport = JSON.parse(stdout) as {
        sha256: string;
        source_index: number;
        source_id: string;
        source_name: string;
        synthesis_model_hint?: string;
        regen_model_id: string;
        regen_model_label: string;
        voicing?: Record<string, unknown>;
        original_audio_opened?: boolean;
        samples: number;
        sample_rate: number;
        note_count: number;
        spectral: {
          peak_dbfs: number | null;
          rms_dbfs: number | null;
          spectral_centroid_hz: number;
          low_band_pct: number;
          mid_band_pct: number;
          high_band_pct: number;
        };
      };

      const durationS = rawCues.samples / rawCues.rate;
      const pitchRes = await runRust(SC_QA, [
        'guitar-pitch-check',
        rec.sourceWav,
        dryWavPath,
        '0',
        String(durationS),
      ]);
      const pitchCheck = JSON.parse(pitchRes.stdout) as IndependentPitchReport;

      setBoundedSessionMap(rec, rec.editedCues, runId, cuesPath);
      setBoundedSessionMap(rec, rec.editedDryWavs, runId, dryWavPath);
      rec.lastOutputWav = dryWavPath;
      rec.lastOutputLabel = `Stage 1 Dry Regeneration · ${renderReport.source_name}`;

      res.json({
        runId,
        drySha256: renderReport.sha256,
        sourceIndex: renderReport.source_index,
        sourceId: renderReport.source_id,
        sourceName: renderReport.source_name,
        synthesisModelHint: renderReport.synthesis_model_hint,
        regenModelId: renderReport.regen_model_id,
        regenModelLabel: renderReport.regen_model_label,
        voicing: renderReport.voicing,
        original_audio_opened: renderReport.original_audio_opened ?? false,
        samples: renderReport.samples,
        sampleRate: renderReport.sample_rate,
        noteCount: renderReport.note_count,
        spectral: renderReport.spectral,
        dryWavUrl: registerMedia(dryWavPath, 'audio/wav', `dry-source-${sourceIndex}-${runId}.wav`),
        cuesJsonUrl: registerMedia(cuesPath, 'application/json', `cues-${runId}.json`),
        pitchCheck,
      });
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to regenerate dry source.') });
    } finally {
      if (voicingPath && fs.existsSync(voicingPath)) {
        try {
          fs.unlinkSync(voicingPath);
        } catch {
          // ignore temp cleanup failure
        }
      }
    }
  });

  app.post('/api/sessions/:id/dry-region-edit', async (req: Request, res: Response) => {
    let cfgPath: string | null = null;
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }
      const cuesRunId = typeof req.body?.cuesRunId === 'string' ? req.body.cuesRunId : '';
      const cuesPath =
        (cuesRunId && rec.editedCues.get(cuesRunId)) || path.join(rec.verifyDir, 'cues.json');
      const cachedDryPath =
        (cuesRunId && rec.editedDryWavs.get(cuesRunId)) ||
        path.join(rec.verifyDir, 'dry-candidate.wav');
      if (!fs.existsSync(cuesPath) || !fs.existsSync(cachedDryPath)) {
        res.status(400).json({ error: 'Cached dry take or extracted cues JSON not found.' });
        return;
      }

      const commit = req.body?.commit === true;
      const sourceIndex = Math.max(0, Math.floor(Number(req.body?.sourceIndex ?? 1) || 0));
      const edit = req.body?.edit ?? {};
      const voicing = req.body?.voicing ?? undefined;

      const runId = crypto.randomUUID().slice(0, 8);
      const runDir = path.join(rec.workDir, `region-edit-${runId}`);
      ensurePrivateDir(runDir);
      cfgPath = path.join(runDir, `region-cfg-${runId}.json`);
      const outEdited = path.join(runDir, `dry-edited-${runId}.wav`);
      const outBefore = path.join(runDir, `region-before-${runId}.wav`);
      const outAfter = path.join(runDir, `region-after-${runId}.wav`);

      fs.writeFileSync(
        cfgPath,
        JSON.stringify({ source_index: sourceIndex, voicing, edit }),
        { encoding: 'utf8', mode: 0o600 },
      );

      const { stdout } = await runRust(SC_QA, [
        'dry-region-edit',
        cuesPath,
        cachedDryPath,
        cfgPath,
        outEdited,
        outBefore,
        outAfter,
      ]);
      const report = JSON.parse(stdout) as Record<string, unknown>;

      if (commit) {
        setBoundedSessionMap(rec, rec.editedCues, runId, cuesPath);
        setBoundedSessionMap(rec, rec.editedDryWavs, runId, outEdited);
        rec.lastOutputWav = outEdited;
        rec.lastOutputLabel = `Stage 2 Dry Region Edit (${runId})`;
      } else {
        setBoundedSessionMap(rec, rec.auxRuns, `region-preview-${runId}`, runDir);
      }

      res.json({
        runId,
        committed: commit,
        editedWavUrl: registerMedia(outEdited, 'audio/wav', `dry-region-edited-${runId}.wav`),
        beforeSliceUrl: registerMedia(outBefore, 'audio/wav', `region-before-${runId}.wav`),
        afterSliceUrl: registerMedia(outAfter, 'audio/wav', `region-after-${runId}.wav`),
        ...report,
      });
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to run dry region edit.') });
    } finally {
      if (cfgPath && fs.existsSync(cfgPath)) {
        try {
          fs.unlinkSync(cfgPath);
        } catch {
          // ignore temp cleanup failure
        }
      }
    }
  });

  app.post('/api/sessions/:id/pipeline-render', async (req: Request, res: Response) => {
    let cfgPath: string | null = null;
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }
      const cuesRunId = typeof req.body?.cuesRunId === 'string' ? req.body.cuesRunId : '';
      const cachedDryPath =
        (cuesRunId && rec.editedDryWavs.get(cuesRunId)) ||
        path.join(rec.verifyDir, 'dry-candidate.wav');
      if (!fs.existsSync(cachedDryPath)) {
        res.status(400).json({ error: 'Cached dry take WAV not found for session.' });
        return;
      }

      const runId = crypto.randomUUID().slice(0, 8);
      const pipeDir = path.join(rec.workDir, 'pipeline');
      ensurePrivateDir(pipeDir);
      cfgPath = path.join(pipeDir, `pipeline-cfg-${runId}.json`);
      const outWavPath = path.join(pipeDir, `pipeline-out-${runId}.wav`);

      const payload = {
        mic_enabled: Boolean(req.body?.micEnabled),
        room_enabled: Boolean(req.body?.roomEnabled),
        rig: req.body?.rigConfig ?? {},
        plugin_slot: req.body?.pluginSlot ?? { enabled: false, plugin_id: 'none' },
      };
      fs.writeFileSync(cfgPath, JSON.stringify(payload), { encoding: 'utf8', mode: 0o600 });

      const { stdout } = await runRust(SC_QA, ['pipeline-render', cachedDryPath, cfgPath, outWavPath]);
      const report = JSON.parse(stdout) as Record<string, unknown>;
      const activeParts = ['Stage 1 Dry'];
      if (payload.mic_enabled) activeParts.push('Stage 3 Mic');
      if (payload.room_enabled) activeParts.push('Stage 4 Room');
      if (payload.plugin_slot?.enabled && payload.plugin_slot?.plugin_id !== 'none') {
        activeParts.push(`Stage 5 Plugin (${String(payload.plugin_slot.plugin_id)})`);
      }
      setBoundedSessionMap(rec, rec.pipelineOutputs, runId, outWavPath);
      rec.lastOutputWav = outWavPath;
      rec.lastOutputLabel = activeParts.join(' -> ');
      res.json({
        runId,
        wavUrl: registerMedia(outWavPath, 'audio/wav', `${rec.id}-pipeline-${runId}.wav`),
        ...report,
      });
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to render pipeline audio.') });
    } finally {
      if (cfgPath && fs.existsSync(cfgPath)) {
        try {
          fs.unlinkSync(cfgPath);
        } catch {
          // ignore temp cleanup failure
        }
      }
    }
  });

  app.post('/api/sessions/:id/layers', async (req: Request, res: Response) => {
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }

      const startS = Number(req.body?.startS ?? 20.0);
      const endS = Number(req.body?.endS ?? 27.0);
      const faintRidges = Boolean(req.body?.faintRidges);

      const srcBytes = fs.readFileSync(rec.sourceWav);
      const sliced = sliceMonoWavBuffer(srcBytes, startS, endS);

      const runId = crypto.randomUUID().slice(0, 8);
      const runDir = path.join(rec.workDir, `layers-${runId}`);
      ensurePrivateDir(runDir);
      setBoundedSessionMap(rec, rec.auxRuns, `layers-${runId}`, runDir);
      const windowWav = path.join(runDir, 'window.wav');
      const analysisJson = path.join(runDir, 'analysis.json');
      const renderDir = path.join(runDir, 'stems');
      fs.writeFileSync(windowWav, sliced.buffer, { flag: 'wx', mode: 0o600 });

      const analyzeCmd = faintRidges ? 'analyze-layers-faint' : 'analyze-layers';
      await runRust(SC_BIN, [analyzeCmd, windowWav, analysisJson]);
      await runRust(SC_BIN, ['render-layers', analysisJson, renderDir]);

      const payload: LayerSessionResult = {
        runId,
        startS: sliced.startS,
        endS: sliced.endS,
        faintRidges,
        mixUrl: registerMedia(path.join(renderDir, 'mix.wav'), 'audio/wav', `layers-mix-${runId}.wav`),
        analysisJsonUrl: registerMedia(analysisJson, 'application/json', `layers-analysis-${runId}.json`),
        stems: [
          {
            id: 'pitch',
            label: 'Pitch',
            description: 'Coherent fundamental & harmonic ridges',
            url: registerMedia(path.join(renderDir, 'pitch.wav'), 'audio/wav', `stem-pitch-${runId}.wav`),
          },
          {
            id: 'ringing',
            label: 'Ringing',
            description: 'Resonant sustain & body modes',
            url: registerMedia(path.join(renderDir, 'ring.wav'), 'audio/wav', `stem-ring-${runId}.wav`),
          },
          {
            id: 'texture',
            label: 'Texture',
            description: 'Broadband pick / air / non-tonal residual',
            url: registerMedia(path.join(renderDir, 'texture.wav'), 'audio/wav', `stem-texture-${runId}.wav`),
          },
          {
            id: 'breath_accent',
            label: 'Breath Accent',
            description: 'Localized transient / breath accent cue',
            url: registerMedia(
              path.join(renderDir, 'accent.wav'),
              'audio/wav',
              `stem-accent-${runId}.wav`,
            ),
          },
        ],
      };

      res.json(payload);
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to render editable layers.') });
    }
  });

  app.post('/api/sessions/:id/repair-gap', async (req: Request, res: Response) => {
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }

      const verifyPath = path.join(rec.verifyDir, 'verification.json');
      const verification = JSON.parse(fs.readFileSync(verifyPath, 'utf8')) as VerificationReport;
      const startS = Number(req.body?.startS);
      const gapMs = Number(req.body?.gapMs);

      const check = validateGapRepairRange(
        verification.sample_rate,
        verification.samples,
        startS,
        gapMs,
      );
      if (!check.ok) {
        res.status(400).json({ error: check.error });
        return;
      }

      const runId = crypto.randomUUID().slice(0, 8);
      const runDir = path.join(rec.workDir, `gap-repair-${runId}`);
      ensurePrivateDir(runDir);
      setBoundedSessionMap(rec, rec.auxRuns, `gap-${runId}`, runDir);
      const outWav = path.join(runDir, 'repaired-gap.wav');

      try {
        const { stdout } = await runRust(SC_BIN, [
          'repair-gap',
          rec.sourceWav,
          outWav,
          String(check.startFrame),
          String(check.endFrame),
        ]);
        const [srcHashRes, outHashRes] = await Promise.all([
          runRust(SC_QA, ['hash', rec.sourceWav]),
          runRust(SC_QA, ['hash', outWav]),
        ]);
        const currentSrcHash = srcHashRes.stdout.trim().split(/\s+/)[0] ?? '';
        const outHash = outHashRes.stdout.trim().split(/\s+/)[0] ?? '';
        const payload: GapRepairResult = {
          runId,
          status: 'filled',
          startS: check.startFrame / verification.sample_rate,
          gapMs: (check.repairedFrames / verification.sample_rate) * 1000,
          startFrame: check.startFrame,
          endFrame: check.endFrame,
          repairedFrames: check.repairedFrames,
          sampleRate: verification.sample_rate,
          sourceSha256: currentSrcHash,
          repairedSha256: outHash,
          repairedWavUrl: registerMedia(outWav, 'audio/wav', `gap-repaired-${runId}.wav`),
          unaffectedPreserved: currentSrcHash === verification.source_sha256,
          message: stdout.trim() || 'Rust repair-gap replaced only the explicit interval and copied all outside samples unchanged.',
        };
        res.json(payload);
      } catch (rustErr) {
        const msg = sanitizeClientError(rustErr, 'Gap repair declined by Rust engine.');
        const payload: GapRepairResult = {
          runId,
          status: 'declined',
          startS: check.startFrame / verification.sample_rate,
          gapMs: (check.repairedFrames / verification.sample_rate) * 1000,
          startFrame: check.startFrame,
          endFrame: check.endFrame,
          repairedFrames: check.repairedFrames,
          sampleRate: verification.sample_rate,
          sourceSha256: verification.source_sha256,
          repairedSha256: null,
          repairedWavUrl: null,
          unaffectedPreserved: true,
          message: msg,
        };
        res.json(payload);
      }
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to run gap repair.') });
    }
  });

  async function loadStudioCatalogsData(): Promise<unknown> {
    const qaBin = resolveRustBinary('spectracraft-qa');
    if (fs.existsSync(qaBin)) {
      try {
        const { stdout } = await runRust(qaBin, ['studio-catalogs']);
        return JSON.parse(stdout);
      } catch {
        // Fall through to authoritative local capture-reference JSON files.
      }
    }
    return loadLocalStudioCatalogs(
      CAPTURE_REFERENCE_DIR,
      process.env.SPECTRAL_SOURCE_CATALOG_PATH,
    );
  }

  app.get('/api/studio/catalogs', async (_req: Request, res: Response) => {
    try {
      if (!cachedStudioCatalogs) {
        cachedStudioCatalogs = await loadStudioCatalogsData();
      }
      res.json(cachedStudioCatalogs);
    } catch (err) {
      const status = err instanceof CatalogLoadError ? err.statusCode : 503;
      res.status(status).json({ error: sanitizeClientError(err, 'Failed to load studio catalogs.') });
    }
  });

  app.post('/api/studio/reload-catalogs', async (_req: Request, res: Response) => {
    try {
      cachedStudioCatalogs = null;
      cachedStudioCatalogs = await loadStudioCatalogsData();
      res.json(cachedStudioCatalogs);
    } catch (err) {
      const status = err instanceof CatalogLoadError ? err.statusCode : 503;
      res.status(status).json({ error: sanitizeClientError(err, 'Failed to reload studio catalogs.') });
    }
  });

  app.post('/api/studio/analyze', async (req: Request, res: Response) => {
    let cfgPath: string | null = null;
    try {
      const rigConfig = req.body?.rigConfig ?? req.body ?? {};
      const runId = crypto.randomUUID().slice(0, 8);
      const tmpDir = path.join(SESSIONS_ROOT, 'analysis-tmp');
      ensurePrivateDir(tmpDir);
      cfgPath = path.join(tmpDir, `rig-analysis-${runId}.json`);
      fs.writeFileSync(cfgPath, JSON.stringify(rigConfig), { encoding: 'utf8', mode: 0o600 });
      const { stdout } = await runRust(SC_QA, ['studio-analysis', cfgPath]);
      res.json(JSON.parse(stdout));
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to analyze studio rig.') });
    } finally {
      if (cfgPath && fs.existsSync(cfgPath)) {
        try {
          fs.unlinkSync(cfgPath);
        } catch {
          // ignore cleanup failure
        }
      }
    }
  });

  app.post('/api/sessions/:id/capture', async (req: Request, res: Response) => {
    let cfgPath: string | null = null;
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }
      if (req.body?.dryApproved !== true) {
        res.status(400).json({
          error: 'Stage 3 Dry Listening Gate (5/5) must pass before Stage 4 virtual capture.',
        });
        return;
      }
      const cuesRunId = typeof req.body?.cuesRunId === 'string' ? req.body.cuesRunId : '';
      const dryWavPath =
        (cuesRunId && rec.editedDryWavs.get(cuesRunId)) ||
        path.join(rec.verifyDir, 'dry-candidate.wav');
      if (!fs.existsSync(dryWavPath)) {
        res.status(400).json({ error: 'Dry candidate WAV not found for session.' });
        return;
      }

      const rigConfig = req.body?.rigConfig ?? {};
      const runId = crypto.randomUUID().slice(0, 8);
      const captureDir = path.join(rec.workDir, 'captures');
      ensurePrivateDir(captureDir);
      cfgPath = path.join(captureDir, `capture-cfg-${runId}.json`);
      const outWavPath = path.join(captureDir, `capture-${runId}.wav`);
      fs.writeFileSync(cfgPath, JSON.stringify(rigConfig), { encoding: 'utf8', mode: 0o600 });

      const { stdout } = await runRust(SC_QA, ['studio-capture', dryWavPath, cfgPath, outWavPath]);
      const report = JSON.parse(stdout) as Record<string, unknown>;
      setBoundedSessionMap(rec, rec.auxRuns, `capture-${runId}`, outWavPath);
      rec.lastOutputWav = outWavPath;
      rec.lastOutputLabel = `Stage 4 Studio Capture (${runId})`;
      res.json({
        runId,
        wavUrl: registerMedia(outWavPath, 'audio/wav', `${rec.id}-capture-${runId}.wav`),
        ...report,
      });
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to render studio capture.') });
    } finally {
      if (cfgPath && fs.existsSync(cfgPath)) {
        try {
          fs.unlinkSync(cfgPath);
        } catch {
          // ignore temp cleanup failure
        }
      }
    }
  });

  app.post('/api/sessions/:id/prompt-render', async (req: Request, res: Response) => {
    let cfgPath: string | null = null;
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }
      if (req.body?.dryApproved !== true) {
        res.status(400).json({
          error: 'Stage 3 Dry Listening Gate (5/5) must pass before Stage 4 prompt-render.',
        });
        return;
      }
      const cuesRunId = typeof req.body?.cuesRunId === 'string' ? req.body.cuesRunId : '';
      const cuesPath =
        (cuesRunId && rec.editedCues.get(cuesRunId)) || path.join(rec.verifyDir, 'cues.json');
      if (!fs.existsSync(cuesPath)) {
        res.status(400).json({ error: 'Extracted cues JSON not found for session.' });
        return;
      }

      const rigConfig = req.body?.rigConfig ?? {};
      const runId = crypto.randomUUID().slice(0, 8);
      const captureDir = path.join(rec.workDir, 'captures');
      ensurePrivateDir(captureDir);
      cfgPath = path.join(captureDir, `prompt-cfg-${runId}.json`);
      const outWavPath = path.join(captureDir, `prompt-render-${runId}.wav`);
      fs.writeFileSync(cfgPath, JSON.stringify(rigConfig), { encoding: 'utf8', mode: 0o600 });

      const { stdout } = await runRust(SC_QA, [
        'studio-prompt-render',
        cuesPath,
        cfgPath,
        outWavPath,
      ]);
      const report = JSON.parse(stdout) as Record<string, unknown>;
      setBoundedSessionMap(rec, rec.auxRuns, `prompt-${runId}`, outWavPath);
      rec.lastOutputWav = outWavPath;
      rec.lastOutputLabel = `Stage 4 Prompt Render (${runId})`;
      res.json({
        runId,
        wavUrl: registerMedia(outWavPath, 'audio/wav', `${rec.id}-prompt-render-${runId}.wav`),
        ...report,
      });
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to run prompt render.') });
    } finally {
      if (cfgPath && fs.existsSync(cfgPath)) {
        try {
          fs.unlinkSync(cfgPath);
        } catch {
          // ignore temp cleanup failure
        }
      }
    }
  });

  app.post('/api/sessions/:id/variants', async (req: Request, res: Response) => {
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }
      if (req.body?.dryApproved !== true) {
        res.status(400).json({
          error: 'Stage 3 Dry Listening Gate (5/5) must pass before rendering capture variants.',
        });
        return;
      }
      const selection = parseSelectionStrict(String(req.body?.selection ?? 'all'));
      const cuesRunId = typeof req.body?.cuesRunId === 'string' ? req.body.cuesRunId : '';
      const cuesPath =
        (cuesRunId && rec.editedCues.get(cuesRunId)) || path.join(rec.verifyDir, 'cues.json');

      const runId = crypto.randomUUID().slice(0, 8);
      const outDir = path.join(rec.workDir, `variants-${runId}`);
      await runRust(SC_QA, [
        'guitar-variants',
        rec.sourceWav,
        cuesPath,
        outDir,
        selection,
        '--human-approved',
      ]);
      rec.variantsDir = outDir;
      setBoundedSessionMap(rec, rec.auxRuns, `variants-${runId}`, outDir);

      const manifest = hydrateVariants(outDir);
      if (!manifest) {
        throw new Error('Rust guitar-variants did not produce variants-manifest.json');
      }
      res.json(manifest);
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to render capture variants.') });
    }
  });

  app.post('/api/sessions/:id/technical-report', async (req: Request, res: Response) => {
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }

      const hasExplicitRunSelector =
        req.body?.pipelineRunId !== undefined || req.body?.cuesRunId !== undefined;
      const pipelineRunId =
        typeof req.body?.pipelineRunId === 'string' ? req.body.pipelineRunId.trim() : '';
      const cuesRunId = typeof req.body?.cuesRunId === 'string' ? req.body.cuesRunId.trim() : '';
      const rawTitle =
        typeof req.body?.title === 'string' && req.body.title.trim().length > 0
          ? req.body.title.trim()
          : `${rec.label} — Blind Before/After Technical Audit`;
      const safeTitle = rawTitle.replace(/[\r\n\x00-\x1f\x7f]/g, ' ').slice(0, 140);

      let candidateAfter = path.join(rec.verifyDir, 'dry-candidate.wav');
      if (pipelineRunId) {
        const pipeOut = rec.pipelineOutputs.get(pipelineRunId);
        if (pipeOut && fs.existsSync(pipeOut)) {
          candidateAfter = pipeOut;
        }
      } else if (cuesRunId) {
        const edited = rec.editedDryWavs.get(cuesRunId);
        if (edited && fs.existsSync(edited)) {
          candidateAfter = edited;
        }
      } else if (!hasExplicitRunSelector && rec.lastOutputWav && fs.existsSync(rec.lastOutputWav)) {
        candidateAfter = rec.lastOutputWav;
      }

      if (!fs.existsSync(rec.sourceWav) || !fs.existsSync(candidateAfter)) {
        res.status(400).json({ error: 'Both raw input WAV and rendered output WAV are required.' });
        return;
      }

      const rawPipelineLabel =
        typeof req.body?.pipelineLabel === 'string' && req.body.pipelineLabel.trim().length > 0
          ? req.body.pipelineLabel.trim()
          : rec.lastOutputLabel ?? 'Stage 1 Dry Regeneration (post-dry stages bypassed)';
      const safePipelineLabel = rawPipelineLabel
        .replace(/[\r\n\x00-\x1f\x7f]/g, ' ')
        .slice(0, 180);

      const reportId = crypto.randomUUID().slice(0, 10);
      const reportDir = path.join(rec.workDir, 'reports');
      ensurePrivateDir(reportDir);
      const reportJsonPath = path.join(reportDir, `technical-report-${reportId}.json`);

      await runRust(SC_QA, [
        'technical-report',
        rec.sourceWav,
        candidateAfter,
        reportJsonPath,
        safeTitle,
        safePipelineLabel,
      ]);

      const report = JSON.parse(fs.readFileSync(reportJsonPath, 'utf8')) as Record<string, unknown>;
      setBoundedSessionMap(rec, rec.reports, reportId, reportJsonPath);

      res.json({
        reportId,
        report,
      });
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to generate technical report.') });
    }
  });

  app.post('/api/sessions/:id/technical-report/export', async (req: Request, res: Response) => {
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }

      const reportId = typeof req.body?.reportId === 'string' ? req.body.reportId : '';
      const reportJsonPath = reportId ? rec.reports.get(reportId) : null;
      if (!reportJsonPath || !isConfinedPath(rec.workDir, reportJsonPath) || !fs.existsSync(reportJsonPath)) {
        res.status(404).json({ error: 'Technical report not found. Generate a report first.' });
        return;
      }

      const rawFormat = String(req.body?.format ?? 'pdf').trim().toLowerCase();
      if (rawFormat !== 'pdf' && rawFormat !== 'markdown' && rawFormat !== 'docx') {
        res.status(400).json({ error: 'Supported export formats: pdf, markdown, docx.' });
        return;
      }

      const reportDir = path.join(rec.workDir, 'reports');
      ensurePrivateDir(reportDir);
      const exportId = crypto.randomUUID().slice(0, 8);
      const ext = rawFormat === 'pdf' ? 'pdf' : 'md';
      const outPath = path.join(reportDir, `spectracraft-report-${reportId}-${exportId}.${ext}`);

      const { stdout } = await runRust(SC_QA, ['export-report', reportJsonPath, outPath, rawFormat]);
      const receipt = JSON.parse(stdout) as {
        requested_format: string;
        actual_format: string;
        extension: string;
        content_type: string;
        engine: string;
        fallback_used: boolean;
        fallback_note: string | null;
        bytes: number;
        sha256: string;
      };

      const safeStem = sanitizeDownloadName(rec.label.replace(/\.wav$/i, ''));
      const downloadFilename = `${safeStem}-technical-report.${receipt.extension || ext}`;
      const url = registerMedia(
        outPath,
        receipt.content_type || (ext === 'pdf' ? 'application/pdf' : 'text/markdown; charset=utf-8'),
        downloadFilename,
      );

      res.json({
        url,
        downloadUrl: `${url}?download=1`,
        filename: downloadFilename,
        receipt,
      });
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to export technical report.') });
    }
  });

  app.post('/api/sessions/:id/daw-export', async (req: Request, res: Response) => {
    try {
      const id = String(req.params.id);
      if (id === 'slow-solo-ref') {
        await ensureDefaultSession();
      }
      const rec = getSession(id);
      if (!rec) {
        res.status(404).json({ error: 'Session not found.' });
        return;
      }

      const targetRaw = String(req.body?.target ?? 'daw').trim().toLowerCase();
      if (!['audition', 'logic', 'daw', 'soundcraft'].includes(targetRaw)) {
        res.status(400).json({ error: 'Supported DAW targets: audition, logic, daw, soundcraft.' });
        return;
      }
      const bitDepth = String(req.body?.bitDepth ?? '24').trim() === '32' ? '32' : '24';

      const hasExplicitRunSelector =
        req.body?.pipelineRunId !== undefined || req.body?.cuesRunId !== undefined;
      const cuesRunId = typeof req.body?.cuesRunId === 'string' ? req.body.cuesRunId.trim() : '';
      const pipelineRunId =
        typeof req.body?.pipelineRunId === 'string' ? req.body.pipelineRunId.trim() : '';

      const dryWav =
        (cuesRunId && rec.editedDryWavs.get(cuesRunId)) ||
        path.join(rec.verifyDir, 'dry-candidate.wav');
      if (!fs.existsSync(dryWav)) {
        res.status(400).json({ error: 'Dry take WAV (dry-candidate.wav) not found for this session.' });
        return;
      }

      let finalWav = dryWav;
      if (pipelineRunId) {
        const pipeOut = rec.pipelineOutputs.get(pipelineRunId);
        if (pipeOut && fs.existsSync(pipeOut)) {
          finalWav = pipeOut;
        }
      } else if (!hasExplicitRunSelector && rec.lastOutputWav && fs.existsSync(rec.lastOutputWav)) {
        finalWav = rec.lastOutputWav;
      }

      const cuesJson =
        (cuesRunId && rec.editedCues.get(cuesRunId)) || path.join(rec.verifyDir, 'cues.json');
      const cuesArg = fs.existsSync(cuesJson) ? cuesJson : '-';

      const exportId = crypto.randomUUID().slice(0, 8);
      const outDir = path.join(rec.workDir, 'daw-interchange', `${targetRaw}-${exportId}`);
      ensurePrivateDir(outDir);
      setBoundedSessionMap(rec, rec.auxRuns, `daw-${exportId}`, outDir);

      const { stdout } = await runRust(SC_QA, [
        'export-daw',
        targetRaw,
        dryWav,
        finalWav,
        outDir,
        cuesArg,
        rec.label,
        bitDepth,
      ]);

      const receipt = JSON.parse(stdout) as {
        schema_version: number;
        target_id: string;
        target_name: string;
        session_title: string;
        sample_rate: number;
        bit_depth: string;
        stem_count: number;
        marker_count: number;
        original_audio_opened: boolean;
        assets: Array<{ role: string; relative_path: string; sha256: string; bytes: number }>;
      };

      const contentTypeFor = (rel: string): string => {
        const lower = rel.toLowerCase();
        if (lower.endsWith('.wav')) return 'audio/wav';
        if (lower.endsWith('.csv')) return 'text/csv; charset=utf-8';
        if (lower.endsWith('.sesx') || lower.endsWith('.xml')) return 'application/xml; charset=utf-8';
        return 'application/json; charset=utf-8';
      };

      const files = receipt.assets.map((asset) => {
        const abs = path.resolve(outDir, asset.relative_path);
        const url = isConfinedPath(outDir, abs)
          ? registerMedia(abs, contentTypeFor(asset.relative_path), path.basename(asset.relative_path))
          : null;
        return {
          ...asset,
          url,
          downloadUrl: url ? `${url}?download=1` : null,
        };
      });

      res.json({
        ...receipt,
        files,
      });
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to export DAW interchange bundle.') });
    }
  });

  app.get('/api/audit', async (_req: Request, res: Response) => {
    try {
      const data = cachedAudit ?? (await runRustAudit());
      res.json(data);
    } catch (err) {
      res.status(500).json({ error: sanitizeClientError(err, 'Audit check failed.') });
    }
  });

  app.post('/api/audit/run', async (_req: Request, res: Response) => {
    try {
      const data = await runRustAudit();
      res.json(data);
    } catch (err) {
      res.status(500).json({ error: sanitizeClientError(err, 'Audit check failed.') });
    }
  });

  app.get('/api/settings', async (_req: Request, res: Response) => {
    try {
      const { stdout } = await runRust(SC_QA, ['settings-get'], 30_000);
      res.json(JSON.parse(stdout));
    } catch (err) {
      res.status(500).json({ error: sanitizeClientError(err, 'Failed to load settings.') });
    }
  });

  app.put('/api/settings', async (req: Request, res: Response) => {
    let tmpPath: string | null = null;
    try {
      const payload = req.body && typeof req.body === 'object' ? req.body : null;
      if (!payload) {
        res.status(400).json({ error: 'Expected JSON settings payload.' });
        return;
      }
      const settingsTmpDir = path.join(SESSIONS_ROOT, 'settings-tmp');
      ensurePrivateDir(settingsTmpDir);
      tmpPath = path.join(settingsTmpDir, `settings-${crypto.randomUUID().slice(0, 10)}.json`);
      fs.writeFileSync(tmpPath, JSON.stringify(payload), { encoding: 'utf8', mode: 0o600 });
      const { stdout } = await runRust(SC_QA, ['settings-set', tmpPath], 30_000);
      res.json(JSON.parse(stdout));
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Failed to update settings.') });
    } finally {
      if (tmpPath && fs.existsSync(tmpPath)) {
        try {
          fs.unlinkSync(tmpPath);
        } catch {
          // ignore temp cleanup failure
        }
      }
    }
  });

  app.post('/api/settings/reset', async (_req: Request, res: Response) => {
    try {
      const { stdout } = await runRust(SC_QA, ['settings-reset'], 30_000);
      res.json(JSON.parse(stdout));
    } catch (err) {
      res.status(500).json({ error: sanitizeClientError(err, 'Failed to reset settings.') });
    }
  });

  app.get('/api/updates/check', async (req: Request, res: Response) => {
    try {
      const onlineParam = String(req.query.online ?? '1').trim().toLowerCase();
      const online = onlineParam !== '0' && onlineParam !== 'false';
      const args = ['update-check', ROOT, ...(online ? ['--online'] : [])];
      const { stdout } = await runRust(SC_QA, args, 45_000);
      res.json(JSON.parse(stdout));
    } catch (err) {
      res.status(500).json({ error: sanitizeClientError(err, 'Failed to check updates.') });
    }
  });

  app.post('/api/updates/action', async (req: Request, res: Response) => {
    try {
      const action = String(req.body?.action ?? '').trim().toLowerCase();
      if (action !== 'not_now' && action !== 'download_later' && action !== 'update_now') {
        res.status(400).json({ error: 'Action must be one of: not_now, download_later, update_now.' });
        return;
      }
      const rawArg =
        action === 'not_now'
          ? String(req.body?.noticeId ?? '').trim()
          : String(req.body?.target ?? '').trim();
      const cleanArg = rawArg.replace(/[\r\n\x00-\x1f\x7f]/g, '').slice(0, 180) || '-';
      const { stdout } = await runRust(SC_QA, ['update-action', ROOT, action, cleanArg], 300_000);
      res.json(JSON.parse(stdout));
    } catch (err) {
      res.status(400).json({ error: sanitizeClientError(err, 'Update action failed.') });
    }
  });

  const HELP_DOCS: Record<string, { title: string; relativePath: string }> = {
    readme: { title: 'README — SpectraCraft Overview', relativePath: 'README.md' },
    spec: { title: 'SPECTRACRAFT_SPEC.md — Authoritative Spec', relativePath: 'SPECTRACRAFT_SPEC.md' },
    'craft-libs': { title: 'CRAFT_LIBS_MAINTENANCE.md — craft-libs & Updates', relativePath: 'CRAFT_LIBS_MAINTENANCE.md' },
    'design-contract': { title: 'DESIGN-CONTRACT.md — Dry Gate & Visual Boundaries', relativePath: 'spectracraft-mono/DESIGN-CONTRACT.md' },
    platform: { title: 'PLATFORM.md — Desktop Platform Notes', relativePath: 'PLATFORM.md' },
    plugin: { title: 'PLUGIN_ARCHITECTURE.md — Engine & Plugin Contract', relativePath: 'PLUGIN_ARCHITECTURE.md' },
  };

  app.get('/api/help/docs/:docId', (req: Request, res: Response) => {
    const docId = String(req.params.docId ?? '').trim().toLowerCase();
    const meta = HELP_DOCS[docId];
    if (!meta) {
      res.status(404).json({ error: 'Unknown documentation topic.' });
      return;
    }
    const fullPath = path.resolve(ROOT, meta.relativePath);
    if (!isConfinedPath(ROOT, fullPath) || !fs.existsSync(fullPath)) {
      res.status(404).json({ error: `Document not found on disk: ${meta.relativePath}` });
      return;
    }
    res.json({
      id: docId,
      title: meta.title,
      path: meta.relativePath,
      content: fs.readFileSync(fullPath, 'utf8'),
    });
  });

  app.get('/api/media/:key', (req: Request, res: Response) => {
    const entry = mediaRegistry.get(String(req.params.key));
    if (!entry || !fs.existsSync(entry.filePath)) {
      res.status(404).json({ error: 'Artifact not found.' });
      return;
    }
    const stat = fs.statSync(entry.filePath);
    res.setHeader('Content-Type', entry.contentType);
    res.setHeader('X-Content-Type-Options', 'nosniff');
    res.setHeader('Accept-Ranges', 'bytes');
    if (entry.contentType === 'image/svg+xml') {
      res.setHeader(
        'Content-Security-Policy',
        "default-src 'none'; style-src 'unsafe-inline'; sandbox",
      );
    }
    if (req.query.download === '1') {
      const safeDownloadName = sanitizeDownloadName(entry.downloadName);
      res.setHeader('Content-Disposition', `attachment; filename="${safeDownloadName}"`);
    }

    const range = req.headers.range;
    if (range && entry.contentType === 'audio/wav') {
      const match = /^bytes=(\d*)-(\d*)$/.exec(range.trim());
      if (!match || (!match[1] && !match[2])) {
        res.status(416);
        res.setHeader('Content-Range', `bytes */${stat.size}`);
        res.end();
        return;
      }
      let start: number;
      let end: number;
      if (!match[1]) {
        const suffixLen = Number.parseInt(match[2], 10);
        if (!Number.isFinite(suffixLen) || suffixLen <= 0 || stat.size === 0) {
          res.status(416);
          res.setHeader('Content-Range', `bytes */${stat.size}`);
          res.end();
          return;
        }
        start = Math.max(0, stat.size - suffixLen);
        end = stat.size - 1;
      } else {
        start = Number.parseInt(match[1], 10);
        end = match[2] ? Math.min(stat.size - 1, Number.parseInt(match[2], 10)) : stat.size - 1;
      }
      if (start < 0 || start > end || start >= stat.size) {
        res.status(416);
        res.setHeader('Content-Range', `bytes */${stat.size}`);
        res.end();
        return;
      }
      res.status(206);
      res.setHeader('Content-Range', `bytes ${start}-${end}/${stat.size}`);
      res.setHeader('Content-Length', String(end - start + 1));
      fs.createReadStream(entry.filePath, { start, end }).pipe(res);
      return;
    }
    res.setHeader('Content-Length', String(stat.size));
    fs.createReadStream(entry.filePath).pipe(res);
  });

  app.all('/api/*', (_req: Request, res: Response) => {
    res.status(404).json({ error: 'API route not found.' });
  });

  if (process.env.NODE_ENV !== 'production') {
    const vite = await createViteServer({
      server: { middlewareMode: true },
      appType: 'spa',
    });
    app.use(vite.middlewares);
  } else {
    const distDir = path.resolve(ROOT, 'dist');
    app.use(express.static(distDir));
    app.get('*', (_req: Request, res: Response) => {
      res.sendFile(path.join(distDir, 'index.html'));
    });
  }

  const port = Number(process.env.PORT || 3000);
  app.listen(port, '0.0.0.0', () => {
    console.log(`SpectraCraft Rust-backed workbench listening on http://0.0.0.0:${port}`);
  });
}

startServer().catch((err) => {
  console.error('Failed to start SpectraCraft server:', err);
  process.exit(1);
});
