import crypto from 'node:crypto';
import { execFile } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { promisify } from 'node:util';
import express from 'express';
import type { Request, Response } from 'express';
import { createServer as createViteServer } from 'vite';
import {
  ALL_VARIANTS,
  countUncertainOverlaps,
  sliceMonoWavBuffer,
  validateGapRepairRange,
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

const execFileAsync = promisify(execFile);

const ROOT = process.cwd();
const CRATE_DIR = path.resolve(ROOT, 'spectracraft-mono');
const SC_BIN = path.resolve(CRATE_DIR, 'target/release/spectracraft-mono');
const SC_QA = path.resolve(CRATE_DIR, 'target/release/spectracraft-qa');

const REF_WAV = '/tmp/slow-solo-ref/slow_solo_48000_mono_exact.wav';
const DEFAULT_VERIFY_DIR = '/tmp/slow-solo-verify-48k-exact';
const DEFAULT_VARIANTS_DIR = '/tmp/slow-solo-variants-48k-exact';
const SESSIONS_ROOT = '/tmp/spectracraft-sessions';
const EMPTY_RETIRED_DIR = '/tmp/empty-retired';

fs.mkdirSync(SESSIONS_ROOT, { recursive: true });
fs.mkdirSync(EMPTY_RETIRED_DIR, { recursive: true });

const RUST_ENV: NodeJS.ProcessEnv = {
  ...process.env,
  PATH: `/tmp/alsa-pc:/tmp/cargo/bin:${process.env.PATH ?? ''}`,
  RUSTUP_HOME: process.env.RUSTUP_HOME ?? '/tmp/rustup',
  CARGO_HOME: process.env.CARGO_HOME ?? '/tmp/cargo',
  PKG_CONFIG_PATH: process.env.PKG_CONFIG_PATH ?? '/tmp/alsa-pc',
};

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
}

const sessions = new Map<string, SessionRecord>();
const mediaRegistry = new Map<string, { filePath: string; contentType: string; downloadName: string }>();
let cachedStudioCatalogs: unknown | null = null;

function registerMedia(filePath: string, contentType: string, downloadName?: string): string {
  const abs = path.resolve(filePath);
  const key = crypto.createHash('sha256').update(abs).digest('hex').slice(0, 24);
  mediaRegistry.set(key, {
    filePath: abs,
    contentType,
    downloadName: downloadName ?? path.basename(abs),
  });
  return `/api/media/${key}`;
}

async function runRust(bin: string, args: string[], timeoutMs = 180_000): Promise<{ stdout: string; stderr: string }> {
  if (!fs.existsSync(bin)) {
    throw new Error(`Compiled Rust binary missing at ${bin}`);
  }
  const { stdout, stderr } = await execFileAsync(bin, args, {
    cwd: ROOT,
    env: RUST_ENV,
    timeout: timeoutMs,
    maxBuffer: 32 * 1024 * 1024,
  });
  return { stdout, stderr };
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
    const wavPath = path.join(variantsDir, entry.file);
    return {
      ...entry,
      url: fs.existsSync(wavPath) ? registerMedia(wavPath, 'audio/wav', entry.file) : undefined,
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

async function ensureDefaultSession(): Promise<SessionRecord> {
  const existing = sessions.get('slow-solo-ref');
  if (existing && fs.existsSync(path.join(existing.verifyDir, 'verification.json'))) {
    return existing;
  }
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
  fs.mkdirSync(workDir, { recursive: true });

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
  };
  sessions.set(record.id, record);
  return record;
}

function getSession(id: string): SessionRecord | null {
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
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  app.post(
    '/api/sessions/upload',
    express.raw({ type: ['audio/*', 'application/octet-stream'], limit: '45mb' }),
    async (req: Request, res: Response) => {
      try {
        const body = req.body as Buffer;
        if (!Buffer.isBuffer(body) || body.length < 44) {
          res.status(400).json({ error: 'Upload a valid mono WAV file (8–96 kHz, up to 90 seconds).' });
          return;
        }
        const rawName = String(req.headers['x-filename'] ?? 'uploaded-audio.wav');
        const safeName = rawName.replace(/[^a-zA-Z0-9._ -]/g, '_').slice(0, 80) || 'uploaded-audio.wav';
        const sessionId = `upload-${crypto.randomUUID().slice(0, 10)}`;
        const workDir = path.join(SESSIONS_ROOT, sessionId);
        fs.mkdirSync(workDir, { recursive: true });
        const sourceWav = path.join(workDir, 'source.wav');
        fs.writeFileSync(sourceWav, body, { flag: 'wx', mode: 0o444 });
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
        };
        sessions.set(sessionId, rec);
        res.json(buildSessionPayload(rec));
      } catch (err) {
        res.status(400).json({
          error:
            err instanceof Error
              ? err.message
              : 'Rust guitar-verify rejected this WAV (requires mono 8–96 kHz, <= 90 s).',
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
      if (!Array.isArray(patches)) {
        res.status(400).json({ error: 'Expected patches array.' });
        return;
      }

      const baseCuesPath = path.join(rec.verifyDir, 'cues.json');
      const rawCues = JSON.parse(fs.readFileSync(baseCuesPath, 'utf8')) as RawCuePerformance;

      for (const patch of patches) {
        const note = rawCues.notes[patch.index];
        if (!note) continue;
        const scale = Math.max(0, Math.min(2.0, Number.isFinite(patch.level_scale) ? patch.level_scale : 1));
        if (Math.abs(scale - 1.0) > 1e-6) {
          note.level_steps = note.level_steps.map((v) => Math.max(0, Math.min(3.85, v * scale)));
        }
        if (Array.isArray(patch.re_picks)) {
          const dur = Math.max(0, note.end - note.start);
          const cleanStrikes = patch.re_picks
            .filter(
              (s) =>
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
      fs.mkdirSync(runDir, { recursive: true });
      const editedCuesPath = path.join(runDir, 'cues-edited.json');
      const dryWavPath = path.join(runDir, 'dry-edited-candidate.wav');
      fs.writeFileSync(editedCuesPath, JSON.stringify(rawCues), { flag: 'wx' });

      // Purely source-free Rust dry render from edited cue JSON alone.
      await runRust(SC_BIN, ['render-dry-guitar', editedCuesPath, dryWavPath]);

      const durationS = rawCues.samples / rawCues.rate;
      const [hashRes, pitchRes] = await Promise.all([
        runRust(SC_QA, ['hash', dryWavPath]),
        runRust(SC_QA, ['guitar-pitch-check', rec.sourceWav, dryWavPath, '0', String(durationS)]),
      ]);

      const pitchReport = JSON.parse(pitchRes.stdout) as IndependentPitchReport;
      const drySha256 = hashRes.stdout.trim().split(/\s+/)[0] ?? '';
      rec.editedCues.set(runId, editedCuesPath);
      rec.editedDryWavs.set(runId, dryWavPath);

      res.json({
        runId,
        drySha256,
        dryWavUrl: registerMedia(dryWavPath, 'audio/wav', `dry-edited-${runId}.wav`),
        cuesJsonUrl: registerMedia(editedCuesPath, 'application/json', `cues-edited-${runId}.json`),
        pitchCheck: pitchReport,
        notes: summarizeNotes(rawCues),
      });
    } catch (err) {
      res.status(400).json({ error: err instanceof Error ? err.message : String(err) });
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
      res.status(400).json({ error: err instanceof Error ? err.message : String(err) });
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
      fs.mkdirSync(runDir, { recursive: true });
      const dryWavPath = path.join(runDir, `dry-source-${sourceIndex}.wav`);

      const args = ['render-dry-source', cuesPath, String(sourceIndex), dryWavPath];
      if (req.body?.voicing && typeof req.body.voicing === 'object') {
        voicingPath = path.join(runDir, `voicing-${runId}.json`);
        fs.writeFileSync(voicingPath, JSON.stringify(req.body.voicing), 'utf8');
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

      rec.editedCues.set(runId, cuesPath);
      rec.editedDryWavs.set(runId, dryWavPath);

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
      res.status(400).json({ error: err instanceof Error ? err.message : String(err) });
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
      fs.mkdirSync(runDir, { recursive: true });
      cfgPath = path.join(runDir, `region-cfg-${runId}.json`);
      const outEdited = path.join(runDir, `dry-edited-${runId}.wav`);
      const outBefore = path.join(runDir, `region-before-${runId}.wav`);
      const outAfter = path.join(runDir, `region-after-${runId}.wav`);

      fs.writeFileSync(
        cfgPath,
        JSON.stringify({ source_index: sourceIndex, voicing, edit }),
        'utf8',
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
        rec.editedCues.set(runId, cuesPath);
        rec.editedDryWavs.set(runId, outEdited);
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
      res.status(400).json({ error: err instanceof Error ? err.message : String(err) });
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
      fs.mkdirSync(pipeDir, { recursive: true });
      cfgPath = path.join(pipeDir, `pipeline-cfg-${runId}.json`);
      const outWavPath = path.join(pipeDir, `pipeline-out-${runId}.wav`);

      const payload = {
        mic_enabled: Boolean(req.body?.micEnabled),
        room_enabled: Boolean(req.body?.roomEnabled),
        rig: req.body?.rigConfig ?? {},
        plugin_slot: req.body?.pluginSlot ?? { enabled: false, plugin_id: 'none' },
      };
      fs.writeFileSync(cfgPath, JSON.stringify(payload), 'utf8');

      const { stdout } = await runRust(SC_QA, ['pipeline-render', cachedDryPath, cfgPath, outWavPath]);
      const report = JSON.parse(stdout) as Record<string, unknown>;
      res.json({
        runId,
        wavUrl: registerMedia(outWavPath, 'audio/wav', `${rec.id}-pipeline-${runId}.wav`),
        ...report,
      });
    } catch (err) {
      res.status(400).json({ error: err instanceof Error ? err.message : String(err) });
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
      fs.mkdirSync(runDir, { recursive: true });
      const windowWav = path.join(runDir, 'window.wav');
      const analysisJson = path.join(runDir, 'analysis.json');
      const renderDir = path.join(runDir, 'stems');
      fs.writeFileSync(windowWav, sliced.buffer, { flag: 'wx' });

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
      res.status(400).json({ error: err instanceof Error ? err.message : String(err) });
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
      fs.mkdirSync(runDir, { recursive: true });
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
        const stderr =
          typeof rustErr === 'object' && rustErr !== null && 'stderr' in rustErr
            ? String((rustErr as { stderr?: unknown }).stderr ?? '')
            : '';
        const msg =
          stderr.trim() || (rustErr instanceof Error ? rustErr.message : String(rustErr));
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
      res.status(400).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  app.get('/api/studio/catalogs', async (_req: Request, res: Response) => {
    try {
      if (!cachedStudioCatalogs) {
        const { stdout } = await runRust(SC_QA, ['studio-catalogs']);
        cachedStudioCatalogs = JSON.parse(stdout);
      }
      res.json(cachedStudioCatalogs);
    } catch (err) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  app.post('/api/studio/reload-catalogs', async (_req: Request, res: Response) => {
    try {
      cachedStudioCatalogs = null;
      const { stdout } = await runRust(SC_QA, ['studio-catalogs']);
      cachedStudioCatalogs = JSON.parse(stdout);
      res.json(cachedStudioCatalogs);
    } catch (err) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  app.post('/api/studio/analyze', async (req: Request, res: Response) => {
    let cfgPath: string | null = null;
    try {
      const rigConfig = req.body?.rigConfig ?? req.body ?? {};
      const runId = crypto.randomUUID().slice(0, 8);
      const tmpDir = path.join(SESSIONS_ROOT, 'analysis-tmp');
      fs.mkdirSync(tmpDir, { recursive: true });
      cfgPath = path.join(tmpDir, `rig-analysis-${runId}.json`);
      fs.writeFileSync(cfgPath, JSON.stringify(rigConfig), 'utf8');
      const { stdout } = await runRust(SC_QA, ['studio-analysis', cfgPath]);
      res.json(JSON.parse(stdout));
    } catch (err) {
      res.status(400).json({ error: err instanceof Error ? err.message : String(err) });
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
      fs.mkdirSync(captureDir, { recursive: true });
      cfgPath = path.join(captureDir, `capture-cfg-${runId}.json`);
      const outWavPath = path.join(captureDir, `capture-${runId}.wav`);
      fs.writeFileSync(cfgPath, JSON.stringify(rigConfig), 'utf8');

      const { stdout } = await runRust(SC_QA, ['studio-capture', dryWavPath, cfgPath, outWavPath]);
      const report = JSON.parse(stdout) as Record<string, unknown>;
      res.json({
        runId,
        wavUrl: registerMedia(outWavPath, 'audio/wav', `${rec.id}-capture-${runId}.wav`),
        ...report,
      });
    } catch (err) {
      res.status(400).json({ error: err instanceof Error ? err.message : String(err) });
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
      fs.mkdirSync(captureDir, { recursive: true });
      cfgPath = path.join(captureDir, `prompt-cfg-${runId}.json`);
      const outWavPath = path.join(captureDir, `prompt-render-${runId}.wav`);
      fs.writeFileSync(cfgPath, JSON.stringify(rigConfig), 'utf8');

      const { stdout } = await runRust(SC_QA, [
        'studio-prompt-render',
        cuesPath,
        cfgPath,
        outWavPath,
      ]);
      const report = JSON.parse(stdout) as Record<string, unknown>;
      res.json({
        runId,
        wavUrl: registerMedia(outWavPath, 'audio/wav', `${rec.id}-prompt-render-${runId}.wav`),
        ...report,
      });
    } catch (err) {
      res.status(400).json({ error: err instanceof Error ? err.message : String(err) });
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

      const manifest = hydrateVariants(outDir);
      if (!manifest) {
        throw new Error('Rust guitar-variants did not produce variants-manifest.json');
      }
      res.json(manifest);
    } catch (err) {
      res.status(400).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  app.get('/api/audit', async (_req: Request, res: Response) => {
    try {
      const data = cachedAudit ?? (await runRustAudit());
      res.json(data);
    } catch (err) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  app.post('/api/audit/run', async (_req: Request, res: Response) => {
    try {
      const data = await runRustAudit();
      res.json(data);
    } catch (err) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  app.get('/api/media/:key', (req: Request, res: Response) => {
    const entry = mediaRegistry.get(String(req.params.key));
    if (!entry || !fs.existsSync(entry.filePath)) {
      res.status(404).json({ error: 'Artifact not found.' });
      return;
    }
    const stat = fs.statSync(entry.filePath);
    res.setHeader('Content-Type', entry.contentType);
    res.setHeader('Accept-Ranges', 'bytes');
    if (req.query.download === '1') {
      res.setHeader('Content-Disposition', `attachment; filename="${entry.downloadName}"`);
    }

    const range = req.headers.range;
    if (range && entry.contentType === 'audio/wav') {
      const match = /bytes=(\d*)-(\d*)/.exec(range);
      if (match) {
        const start = match[1] ? Number.parseInt(match[1], 10) : 0;
        const end = match[2] ? Math.min(stat.size - 1, Number.parseInt(match[2], 10)) : stat.size - 1;
        if (start <= end && start < stat.size) {
          res.status(206);
          res.setHeader('Content-Range', `bytes ${start}-${end}/${stat.size}`);
          res.setHeader('Content-Length', String(end - start + 1));
          fs.createReadStream(entry.filePath, { start, end }).pipe(res);
          return;
        }
      }
    }
    res.setHeader('Content-Length', String(stat.size));
    fs.createReadStream(entry.filePath).pipe(res);
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
