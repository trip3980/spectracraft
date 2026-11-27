/**
 * Typed transport abstraction for SpectraCraft (Audit H1).
 *
 * - Browser / dev mode: `HttpTransport` talks to the local Express bridge (`/api/*`).
 * - Packaged Tauri 2 desktop mode (`window.__TAURI_INTERNALS__`): `TauriTransport` invokes
 *   native Rust Tauri commands directly — zero Node/Express server required.
 */

import type {
  DryRegionEditResponse,
  PipelineRenderResponse,
  RustAcousticAnalysis,
  RustSourceRegenResult,
  RustStudioCatalogs,
} from './studioRig.ts';
import type { ReferenceShape } from './synthesisPrompt.ts';
import type {
  ReportExportResponse,
  TechnicalReportApiResponse,
} from './technicalReport.ts';
import type {
  SettingsSnapshot,
  SpectraCraftSettings,
  UpdateActionKind,
  UpdateActionResult,
  UpdateCheckReport,
} from './settingsAndUpdates.ts';
import type {
  GapRepairResult,
  LayerSessionResult,
  NoteEditPatch,
  SessionPayload,
  VariantsManifest,
} from './workbench.ts';

export type TransportMode = 'http' | 'tauri';

export interface DawInterchangeFile {
  role: string;
  relative_path: string;
  sha256: string;
  bytes: number;
  downloadUrl: string | null;
}

export interface DawInterchangeResult {
  target_id: string;
  target_name: string;
  sample_rate: number;
  bit_depth: string;
  stem_count: number;
  marker_count: number;
  files: DawInterchangeFile[];
}

export interface RenderCuesResponse {
  runId: string;
  dryWavUrl: string;
  cuesJsonUrl: string;
  drySha256?: string;
}

export interface HelpDocResponse {
  id: string;
  title: string;
  path: string;
  content: string;
}

export interface SpectraCraftTransport {
  readonly mode: TransportMode;
  getStudioCatalogs(signal?: AbortSignal): Promise<RustStudioCatalogs>;
  reloadStudioCatalogs(signal?: AbortSignal): Promise<RustStudioCatalogs>;
  analyzeStudioRig(rigConfig: unknown, signal?: AbortSignal): Promise<RustAcousticAnalysis>;
  loadReferenceSession(signal?: AbortSignal): Promise<SessionPayload>;
  uploadWav(fileName: string, wavBytes: ArrayBuffer, signal?: AbortSignal): Promise<SessionPayload>;
  getReferenceShape(
    sessionId: string,
    sourceIndex: number,
    cuesRunId?: string | null,
    signal?: AbortSignal,
  ): Promise<ReferenceShape>;
  analyzeLayers(
    sessionId: string,
    body: { startS: number; endS: number; faintRidges: boolean },
    signal?: AbortSignal,
  ): Promise<LayerSessionResult>;
  renderEditedCues(
    sessionId: string,
    patches: NoteEditPatch[],
    signal?: AbortSignal,
  ): Promise<RenderCuesResponse>;
  regenerateDryWithSource(
    sessionId: string,
    body: { sourceIndex: number; cuesRunId: string | null; voicing: unknown },
    signal?: AbortSignal,
  ): Promise<RustSourceRegenResult>;
  dryRegionEdit(
    sessionId: string,
    body: {
      edit: unknown;
      sourceIndex: number;
      voicing: unknown;
      cuesRunId: string | null;
      commit: boolean;
    },
    signal?: AbortSignal,
  ): Promise<DryRegionEditResponse>;
  pipelineRender(
    sessionId: string,
    body: {
      micEnabled: boolean;
      roomEnabled: boolean;
      rigConfig: unknown;
      pluginSlot: unknown;
      cuesRunId: string | null;
      preampDrive?: number;
    },
    signal?: AbortSignal,
  ): Promise<PipelineRenderResponse>;
  repairGap(
    sessionId: string,
    body: { startS: number; gapMs: number },
    signal?: AbortSignal,
  ): Promise<GapRepairResult>;
  renderVariants(
    sessionId: string,
    body: { selection: string; cuesRunId: string | null; dryApproved: boolean },
    signal?: AbortSignal,
  ): Promise<VariantsManifest>;
  generateTechnicalReport(
    sessionId: string,
    body: {
      cuesRunId: string;
      pipelineRunId: string;
      title: string;
      pipelineLabel: string;
    },
    signal?: AbortSignal,
  ): Promise<TechnicalReportApiResponse>;
  exportTechnicalReport(
    sessionId: string,
    body: { reportId: string; format: 'pdf' | 'markdown' | 'docx' },
    signal?: AbortSignal,
  ): Promise<ReportExportResponse>;
  exportDawInterchange(
    sessionId: string,
    body: {
      target: 'audition' | 'logic' | 'daw' | 'soundcraft';
      bitDepth: '24' | '32';
      cuesRunId: string;
      pipelineRunId: string;
    },
    signal?: AbortSignal,
  ): Promise<DawInterchangeResult>;
  getSettings(signal?: AbortSignal): Promise<SettingsSnapshot>;
  saveSettings(next: SpectraCraftSettings, signal?: AbortSignal): Promise<SettingsSnapshot>;
  resetSettings(signal?: AbortSignal): Promise<SettingsSnapshot>;
  checkUpdates(online: boolean, signal?: AbortSignal): Promise<UpdateCheckReport>;
  runUpdateAction(
    body: { action: UpdateActionKind; noticeId: string; target: string },
    signal?: AbortSignal,
  ): Promise<UpdateActionResult>;
  readMediaBytes(urlOrHandle: string, signal?: AbortSignal): Promise<ArrayBuffer>;
  getHelpDoc(docId: string, signal?: AbortSignal): Promise<HelpDocResponse>;
}

/**
 * Sanitize an arbitrary Unicode filename into a safe ASCII HTTP header fallback while
 * preserving the exact UTF-8 display name in `X-Filename-Encoded` (Audit M6).
 */
export function buildUploadFilenameHeaders(fileName: string): {
  'X-Filename': string;
  'X-Filename-Encoded': string;
} {
  const safeAscii =
    fileName
      .replace(/[^\x20-\x7E]/g, '_')
      .replace(/[\r\n]/g, '')
      .slice(0, 180) || 'upload.wav';
  return {
    'X-Filename': safeAscii,
    'X-Filename-Encoded': encodeURIComponent(fileName),
  };
}

async function parseJsonOrThrow<T>(res: Response, fallbackMsg: string): Promise<T> {
  const body = (await res.json().catch(() => null)) as (T & { error?: string }) | null;
  if (!res.ok) {
    throw new Error(body?.error || `${fallbackMsg} (HTTP ${res.status})`);
  }
  if (body === null) {
    throw new Error(`${fallbackMsg} (empty response)`);
  }
  return body;
}

export class HttpTransport implements SpectraCraftTransport {
  public readonly mode: TransportMode = 'http';
  private readonly baseUrl: string;
  private readonly authToken?: string;

  constructor(baseUrl = '', authToken?: string) {
    this.baseUrl = baseUrl.replace(/\/+$/, '');
    this.authToken = authToken;
  }

  private headers(extra?: Record<string, string>): Record<string, string> {
    const out: Record<string, string> = { ...(extra ?? {}) };
    if (this.authToken) {
      out.Authorization = `Bearer ${this.authToken}`;
    }
    return out;
  }

  private url(path: string): string {
    return `${this.baseUrl}${path}`;
  }

  async getStudioCatalogs(signal?: AbortSignal): Promise<RustStudioCatalogs> {
    const res = await fetch(this.url('/api/studio/catalogs'), { headers: this.headers(), signal });
    return parseJsonOrThrow(res, 'Failed to load studio catalogs');
  }

  async reloadStudioCatalogs(signal?: AbortSignal): Promise<RustStudioCatalogs> {
    const res = await fetch(this.url('/api/studio/reload-catalogs'), {
      method: 'POST',
      headers: this.headers(),
      signal,
    });
    return parseJsonOrThrow(res, 'Catalog reload failed');
  }

  async analyzeStudioRig(rigConfig: unknown, signal?: AbortSignal): Promise<RustAcousticAnalysis> {
    const res = await fetch(this.url('/api/studio/analyze'), {
      method: 'POST',
      headers: this.headers({ 'Content-Type': 'application/json' }),
      body: JSON.stringify({ rigConfig }),
      signal,
    });
    return parseJsonOrThrow(res, 'Studio rig analysis failed');
  }

  async loadReferenceSession(signal?: AbortSignal): Promise<SessionPayload> {
    const res = await fetch(this.url('/api/sessions/default'), { headers: this.headers(), signal });
    return parseJsonOrThrow(res, 'Failed to load reference session');
  }

  async uploadWav(
    fileName: string,
    wavBytes: ArrayBuffer,
    signal?: AbortSignal,
  ): Promise<SessionPayload> {
    const res = await fetch(this.url('/api/sessions/upload'), {
      method: 'POST',
      headers: this.headers({
        'Content-Type': 'application/octet-stream',
        ...buildUploadFilenameHeaders(fileName),
      }),
      body: wavBytes,
      signal,
    });
    return parseJsonOrThrow(res, 'Rust analysis rejected this WAV');
  }

  async getReferenceShape(
    sessionId: string,
    sourceIndex: number,
    cuesRunId?: string | null,
    signal?: AbortSignal,
  ): Promise<ReferenceShape> {
    const qs = new URLSearchParams({ sourceIndex: String(sourceIndex) });
    if (cuesRunId) qs.set('cuesRunId', cuesRunId);
    const res = await fetch(
      this.url(`/api/sessions/${encodeURIComponent(sessionId)}/reference-shape?${qs.toString()}`),
      { headers: this.headers(), signal },
    );
    return parseJsonOrThrow(res, 'Reference shape request failed');
  }

  async analyzeLayers(
    sessionId: string,
    body: { startS: number; endS: number; faintRidges: boolean },
    signal?: AbortSignal,
  ): Promise<LayerSessionResult> {
    const res = await fetch(
      this.url(`/api/sessions/${encodeURIComponent(sessionId)}/layers`),
      {
        method: 'POST',
        headers: this.headers({ 'Content-Type': 'application/json' }),
        body: JSON.stringify(body),
        signal,
      },
    );
    return parseJsonOrThrow(res, 'Layer analysis failed');
  }

  async renderEditedCues(
    sessionId: string,
    patches: NoteEditPatch[],
    signal?: AbortSignal,
  ): Promise<RenderCuesResponse> {
    const res = await fetch(
      this.url(`/api/sessions/${encodeURIComponent(sessionId)}/render-cues`),
      {
        method: 'POST',
        headers: this.headers({ 'Content-Type': 'application/json' }),
        body: JSON.stringify({ patches }),
        signal,
      },
    );
    return parseJsonOrThrow(res, 'Cue render failed');
  }

  async regenerateDryWithSource(
    sessionId: string,
    body: { sourceIndex: number; cuesRunId: string | null; voicing: unknown },
    signal?: AbortSignal,
  ): Promise<RustSourceRegenResult> {
    const res = await fetch(
      this.url(`/api/sessions/${encodeURIComponent(sessionId)}/regenerate-source`),
      {
        method: 'POST',
        headers: this.headers({ 'Content-Type': 'application/json' }),
        body: JSON.stringify(body),
        signal,
      },
    );
    return parseJsonOrThrow(res, 'Source-informed dry regeneration failed');
  }

  async dryRegionEdit(
    sessionId: string,
    body: {
      edit: unknown;
      sourceIndex: number;
      voicing: unknown;
      cuesRunId: string | null;
      commit: boolean;
    },
    signal?: AbortSignal,
  ): Promise<DryRegionEditResponse> {
    const res = await fetch(
      this.url(`/api/sessions/${encodeURIComponent(sessionId)}/dry-region-edit`),
      {
        method: 'POST',
        headers: this.headers({ 'Content-Type': 'application/json' }),
        body: JSON.stringify(body),
        signal,
      },
    );
    return parseJsonOrThrow(res, 'Dry region edit failed');
  }

  async pipelineRender(
    sessionId: string,
    body: {
      micEnabled: boolean;
      roomEnabled: boolean;
      rigConfig: unknown;
      pluginSlot: unknown;
      cuesRunId: string | null;
      preampDrive?: number;
    },
    signal?: AbortSignal,
  ): Promise<PipelineRenderResponse> {
    const res = await fetch(
      this.url(`/api/sessions/${encodeURIComponent(sessionId)}/pipeline-render`),
      {
        method: 'POST',
        headers: this.headers({ 'Content-Type': 'application/json' }),
        body: JSON.stringify(body),
        signal,
      },
    );
    return parseJsonOrThrow(res, 'Rust pipeline render failed');
  }

  async repairGap(
    sessionId: string,
    body: { startS: number; gapMs: number },
    signal?: AbortSignal,
  ): Promise<GapRepairResult> {
    const res = await fetch(
      this.url(`/api/sessions/${encodeURIComponent(sessionId)}/repair-gap`),
      {
        method: 'POST',
        headers: this.headers({ 'Content-Type': 'application/json' }),
        body: JSON.stringify(body),
        signal,
      },
    );
    return parseJsonOrThrow(res, 'Gap repair failed');
  }

  async renderVariants(
    sessionId: string,
    body: { selection: string; cuesRunId: string | null; dryApproved: boolean },
    signal?: AbortSignal,
  ): Promise<VariantsManifest> {
    const res = await fetch(
      this.url(`/api/sessions/${encodeURIComponent(sessionId)}/variants`),
      {
        method: 'POST',
        headers: this.headers({ 'Content-Type': 'application/json' }),
        body: JSON.stringify(body),
        signal,
      },
    );
    return parseJsonOrThrow(res, 'Variant render failed');
  }

  async generateTechnicalReport(
    sessionId: string,
    body: {
      cuesRunId: string;
      pipelineRunId: string;
      title: string;
      pipelineLabel: string;
    },
    signal?: AbortSignal,
  ): Promise<TechnicalReportApiResponse> {
    const res = await fetch(
      this.url(`/api/sessions/${encodeURIComponent(sessionId)}/technical-report`),
      {
        method: 'POST',
        headers: this.headers({ 'Content-Type': 'application/json' }),
        body: JSON.stringify(body),
        signal,
      },
    );
    return parseJsonOrThrow(res, 'Blind technical report generation failed');
  }

  async exportTechnicalReport(
    sessionId: string,
    body: { reportId: string; format: 'pdf' | 'markdown' | 'docx' },
    signal?: AbortSignal,
  ): Promise<ReportExportResponse> {
    const res = await fetch(
      this.url(`/api/sessions/${encodeURIComponent(sessionId)}/technical-report/export`),
      {
        method: 'POST',
        headers: this.headers({ 'Content-Type': 'application/json' }),
        body: JSON.stringify(body),
        signal,
      },
    );
    return parseJsonOrThrow(res, 'Failed to export technical report');
  }

  async exportDawInterchange(
    sessionId: string,
    body: {
      target: 'audition' | 'logic' | 'daw' | 'soundcraft';
      bitDepth: '24' | '32';
      cuesRunId: string;
      pipelineRunId: string;
    },
    signal?: AbortSignal,
  ): Promise<DawInterchangeResult> {
    const res = await fetch(
      this.url(`/api/sessions/${encodeURIComponent(sessionId)}/daw-export`),
      {
        method: 'POST',
        headers: this.headers({ 'Content-Type': 'application/json' }),
        body: JSON.stringify(body),
        signal,
      },
    );
    return parseJsonOrThrow(res, 'DAW interchange export failed');
  }

  async getSettings(signal?: AbortSignal): Promise<SettingsSnapshot> {
    const res = await fetch(this.url('/api/settings'), { headers: this.headers(), signal });
    return parseJsonOrThrow(res, 'Failed to load settings');
  }

  async saveSettings(next: SpectraCraftSettings, signal?: AbortSignal): Promise<SettingsSnapshot> {
    const res = await fetch(this.url('/api/settings'), {
      method: 'PUT',
      headers: this.headers({ 'Content-Type': 'application/json' }),
      body: JSON.stringify(next),
      signal,
    });
    return parseJsonOrThrow(res, 'Failed to persist settings');
  }

  async resetSettings(signal?: AbortSignal): Promise<SettingsSnapshot> {
    const res = await fetch(this.url('/api/settings/reset'), {
      method: 'POST',
      headers: this.headers(),
      signal,
    });
    return parseJsonOrThrow(res, 'Failed to reset settings');
  }

  async checkUpdates(online: boolean, signal?: AbortSignal): Promise<UpdateCheckReport> {
    const res = await fetch(this.url(`/api/updates/check?online=${online ? '1' : '0'}`), {
      headers: this.headers(),
      signal,
    });
    return parseJsonOrThrow(res, 'Update check failed');
  }

  async runUpdateAction(
    body: { action: UpdateActionKind; noticeId: string; target: string },
    signal?: AbortSignal,
  ): Promise<UpdateActionResult> {
    const res = await fetch(this.url('/api/updates/action'), {
      method: 'POST',
      headers: this.headers({ 'Content-Type': 'application/json' }),
      body: JSON.stringify(body),
      signal,
    });
    return parseJsonOrThrow(res, `Update action '${body.action}' failed`);
  }

  async readMediaBytes(urlOrHandle: string, signal?: AbortSignal): Promise<ArrayBuffer> {
    const target = urlOrHandle.startsWith('http') ? urlOrHandle : this.url(urlOrHandle);
    const res = await fetch(target, { headers: this.headers(), signal });
    if (!res.ok) throw new Error(`Failed to fetch media (${res.status})`);
    return res.arrayBuffer();
  }

  async getHelpDoc(docId: string, signal?: AbortSignal): Promise<HelpDocResponse> {
    const res = await fetch(this.url(`/api/help/docs/${encodeURIComponent(docId)}`), {
      headers: this.headers(),
      signal,
    });
    return parseJsonOrThrow(res, 'Failed to load document');
  }
}

export type TauriInvokeFn = <T = unknown>(
  cmd: string,
  args?: Record<string, unknown>,
) => Promise<T>;

/**
 * Native Tauri 2 IPC transport that runs the complete SpectraCraft workstation
 * workflow against Rust commands in `src-tauri/src/lib.rs` with zero Node/Express bridge (Audit H1).
 */
export class TauriTransport implements SpectraCraftTransport {
  public readonly mode: TransportMode = 'tauri';
  private readonly invoke: TauriInvokeFn;

  constructor(invoke: TauriInvokeFn) {
    this.invoke = invoke;
  }

  private async call<T>(cmd: string, args?: Record<string, unknown>, signal?: AbortSignal): Promise<T> {
    if (signal?.aborted) {
      throw new DOMException(`Tauri command '${cmd}' aborted`, 'AbortError');
    }
    const result = await this.invoke<T>(cmd, args);
    if (signal?.aborted) {
      throw new DOMException(`Tauri command '${cmd}' aborted`, 'AbortError');
    }
    return result;
  }

  getStudioCatalogs(signal?: AbortSignal): Promise<RustStudioCatalogs> {
    return this.call('get_studio_catalogs', undefined, signal);
  }

  reloadStudioCatalogs(signal?: AbortSignal): Promise<RustStudioCatalogs> {
    return this.call('get_studio_catalogs', { reload: true }, signal);
  }

  analyzeStudioRig(rigConfig: unknown, signal?: AbortSignal): Promise<RustAcousticAnalysis> {
    return this.call('analyze_studio_rig', { rigConfig }, signal);
  }

  loadReferenceSession(signal?: AbortSignal): Promise<SessionPayload> {
    return this.call('load_reference_session', undefined, signal);
  }

  async uploadWav(
    fileName: string,
    wavBytes: ArrayBuffer,
    signal?: AbortSignal,
  ): Promise<SessionPayload> {
    const bytes = Array.from(new Uint8Array(wavBytes));
    return this.call('import_wav', { fileName, wavBytes: bytes }, signal);
  }

  getReferenceShape(
    sessionId: string,
    sourceIndex: number,
    cuesRunId?: string | null,
    signal?: AbortSignal,
  ): Promise<ReferenceShape> {
    return this.call(
      'get_reference_shape',
      { sessionId, sourceIndex, cuesRunId: cuesRunId ?? null },
      signal,
    );
  }

  analyzeLayers(
    sessionId: string,
    body: { startS: number; endS: number; faintRidges: boolean },
    signal?: AbortSignal,
  ): Promise<LayerSessionResult> {
    return this.call('analyze_layers', { sessionId, ...body }, signal);
  }

  renderEditedCues(
    sessionId: string,
    patches: NoteEditPatch[],
    signal?: AbortSignal,
  ): Promise<RenderCuesResponse> {
    return this.call('render_cues', { sessionId, patches }, signal);
  }

  regenerateDryWithSource(
    sessionId: string,
    body: { sourceIndex: number; cuesRunId: string | null; voicing: unknown },
    signal?: AbortSignal,
  ): Promise<RustSourceRegenResult> {
    return this.call('regenerate_source', { sessionId, ...body }, signal);
  }

  dryRegionEdit(
    sessionId: string,
    body: {
      edit: unknown;
      sourceIndex: number;
      voicing: unknown;
      cuesRunId: string | null;
      commit: boolean;
    },
    signal?: AbortSignal,
  ): Promise<DryRegionEditResponse> {
    return this.call('dry_region_edit', { sessionId, ...body }, signal);
  }

  pipelineRender(
    sessionId: string,
    body: {
      micEnabled: boolean;
      roomEnabled: boolean;
      rigConfig: unknown;
      pluginSlot: unknown;
      cuesRunId: string | null;
      preampDrive?: number;
    },
    signal?: AbortSignal,
  ): Promise<PipelineRenderResponse> {
    return this.call('pipeline_render', { sessionId, ...body }, signal);
  }

  repairGap(
    sessionId: string,
    body: { startS: number; gapMs: number },
    signal?: AbortSignal,
  ): Promise<GapRepairResult> {
    return this.call('repair_gap', { sessionId, ...body }, signal);
  }

  renderVariants(
    sessionId: string,
    body: { selection: string; cuesRunId: string | null; dryApproved: boolean },
    signal?: AbortSignal,
  ): Promise<VariantsManifest> {
    return this.call('render_variants', { sessionId, ...body }, signal);
  }

  generateTechnicalReport(
    sessionId: string,
    body: {
      cuesRunId: string;
      pipelineRunId: string;
      title: string;
      pipelineLabel: string;
    },
    signal?: AbortSignal,
  ): Promise<TechnicalReportApiResponse> {
    return this.call('generate_technical_report', { sessionId, ...body }, signal);
  }

  exportTechnicalReport(
    sessionId: string,
    body: { reportId: string; format: 'pdf' | 'markdown' | 'docx' },
    signal?: AbortSignal,
  ): Promise<ReportExportResponse> {
    return this.call('export_technical_report', { sessionId, ...body }, signal);
  }

  exportDawInterchange(
    sessionId: string,
    body: {
      target: 'audition' | 'logic' | 'daw' | 'soundcraft';
      bitDepth: '24' | '32';
      cuesRunId: string;
      pipelineRunId: string;
    },
    signal?: AbortSignal,
  ): Promise<DawInterchangeResult> {
    return this.call('export_daw_interchange', { sessionId, ...body }, signal);
  }

  getSettings(signal?: AbortSignal): Promise<SettingsSnapshot> {
    return this.call('get_settings', undefined, signal);
  }

  saveSettings(next: SpectraCraftSettings, signal?: AbortSignal): Promise<SettingsSnapshot> {
    return this.call('save_settings', { settings: next }, signal);
  }

  resetSettings(signal?: AbortSignal): Promise<SettingsSnapshot> {
    return this.call('reset_settings', undefined, signal);
  }

  checkUpdates(online: boolean, signal?: AbortSignal): Promise<UpdateCheckReport> {
    return this.call('check_updates', { online }, signal);
  }

  runUpdateAction(
    body: { action: UpdateActionKind; noticeId: string; target: string },
    signal?: AbortSignal,
  ): Promise<UpdateActionResult> {
    return this.call('run_update_action', body, signal);
  }

  async readMediaBytes(urlOrHandle: string, signal?: AbortSignal): Promise<ArrayBuffer> {
    const raw = await this.call<number[]>('read_scoped_media', { handle: urlOrHandle }, signal);
    return new Uint8Array(raw).buffer;
  }

  getHelpDoc(docId: string, signal?: AbortSignal): Promise<HelpDocResponse> {
    return this.call('get_help_doc', { docId }, signal);
  }
}

export function isTauriRuntime(): boolean {
  if (typeof window === 'undefined') return false;
  const w = window as unknown as {
    __TAURI_INTERNALS__?: { invoke?: TauriInvokeFn };
  };
  return typeof w.__TAURI_INTERNALS__?.invoke === 'function';
}

let cachedTransport: SpectraCraftTransport | null = null;

export function getTransport(): SpectraCraftTransport {
  if (cachedTransport) return cachedTransport;
  if (isTauriRuntime()) {
    const invoke = (
      window as unknown as { __TAURI_INTERNALS__: { invoke: TauriInvokeFn } }
    ).__TAURI_INTERNALS__.invoke;
    cachedTransport = new TauriTransport(invoke);
  } else {
    cachedTransport = new HttpTransport('');
  }
  return cachedTransport;
}

export function setTransportForTesting(transport: SpectraCraftTransport | null): void {
  cachedTransport = transport;
}
