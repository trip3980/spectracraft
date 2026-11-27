import test from 'node:test';
import assert from 'node:assert/strict';
import {
  TauriTransport,
  buildUploadFilenameHeaders,
  type TauriInvokeFn,
} from './transport.ts';
import { DEFAULT_SPECTRACRAFT_SETTINGS } from './settingsAndUpdates.ts';

test('M6: buildUploadFilenameHeaders preserves UTF-8 display name and builds ASCII-safe fallback', () => {
  const headers = buildUploadFilenameHeaders('ギター_日本語_café.wav');
  assert.equal(decodeURIComponent(headers['X-Filename-Encoded']), 'ギター_日本語_café.wav');
  assert.match(headers['X-Filename'], /^[\x20-\x7E]+$/);
});

test('H1: TauriTransport runs complete workstation workflow end-to-end with zero Express server', async () => {
  const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
  const invoke: TauriInvokeFn = async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    switch (cmd) {
      case 'get_studio_catalogs':
        return { bands_hz: [125, 250, 500, 1000, 2000, 4000], sources: [], mics: [], surfaces: [] } as unknown as T;
      case 'load_reference_session':
        return {
          id: 'slow-solo-ref',
          label: 'Reference',
          isReference: true,
          urls: {
            sourceWav: 'tauri-media://m-1',
            dryWav: 'tauri-media://m-2',
            cuesJson: 'tauri-media://m-3',
          },
        } as unknown as T;
      case 'import_wav':
        return {
          id: 'upload-1',
          label: String(args?.fileName ?? ''),
          isReference: false,
          urls: {
            sourceWav: 'tauri-media://m-4',
            dryWav: 'tauri-media://m-5',
            cuesJson: 'tauri-media://m-6',
          },
        } as unknown as T;
      case 'get_reference_shape':
        return { confidence: 'high' } as unknown as T;
      case 'render_cues':
        return {
          runId: 'cues-1',
          dryWavUrl: 'tauri-media://m-7',
          cuesJsonUrl: 'tauri-media://m-8',
          drySha256: 'abc123',
        } as unknown as T;
      case 'regenerate_source':
        return {
          runId: 'src-1',
          wavUrl: 'tauri-media://m-9',
          sha256: 'def456',
          original_audio_opened: false,
        } as unknown as T;
      case 'dry_region_edit':
        return {
          runId: 'reg-1',
          editedWavUrl: 'tauri-media://m-10',
          edited_sha256: 'ghi789',
        } as unknown as T;
      case 'pipeline_render':
        return {
          runId: 'pipe-1',
          wavUrl: 'tauri-media://m-11',
          wav_sha256: 'jkl012',
        } as unknown as T;
      case 'generate_technical_report':
        return { reportId: 'rep-1', report: { title: 'Test' } } as unknown as T;
      case 'export_technical_report':
        return { format: 'pdf', downloadUrl: 'tauri-media://m-12', cached: false } as unknown as T;
      case 'export_daw_interchange':
        return {
          target_id: 'audition',
          target_name: 'Adobe Audition',
          sample_rate: 48000,
          bit_depth: '24',
          stem_count: 2,
          marker_count: 6,
          files: [{ role: 'stem', relative_path: 'dry-take.wav', sha256: 'aa', bytes: 100, downloadUrl: 'tauri-media://m-13' }],
        } as unknown as T;
      case 'read_scoped_media':
        assert.ok(String(args?.handle).startsWith('tauri-media://'));
        return [82, 73, 70, 70] as unknown as T; // "RIFF"
      case 'get_settings':
      case 'save_settings':
      case 'reset_settings':
        return { app_id: 'spectracraft', settings: DEFAULT_SPECTRACRAFT_SETTINGS } as unknown as T;
      case 'get_help_doc':
        return { id: 'spec', title: 'Spec', path: 'SPECTRACRAFT_SPEC.md', content: '# Spec' } as unknown as T;
      default:
        throw new Error(`Unexpected Tauri command: ${cmd}`);
    }
  };

  const transport = new TauriTransport(invoke);
  assert.equal(transport.mode, 'tauri');

  await transport.getStudioCatalogs();
  const refSession = await transport.loadReferenceSession();
  assert.equal(refSession.id, 'slow-solo-ref');

  const uploaded = await transport.uploadWav('ギター_take.wav', new Uint8Array([82, 73, 70, 70]).buffer);
  assert.equal(uploaded.label, 'ギター_take.wav');

  await transport.getReferenceShape(uploaded.id, 1, null);
  const cues = await transport.renderEditedCues(uploaded.id, []);
  const src = await transport.regenerateDryWithSource(uploaded.id, {
    sourceIndex: 1,
    cuesRunId: cues.runId,
    voicing: {},
  });
  const reg = await transport.dryRegionEdit(uploaded.id, {
    edit: {},
    sourceIndex: 1,
    voicing: {},
    cuesRunId: src.runId,
    commit: true,
  });
  const pipe = await transport.pipelineRender(uploaded.id, {
    micEnabled: true,
    roomEnabled: true,
    rigConfig: {},
    pluginSlot: {},
    cuesRunId: reg.runId,
  });
  const rep = await transport.generateTechnicalReport(uploaded.id, {
    cuesRunId: reg.runId,
    pipelineRunId: pipe.runId,
    title: 'Packaged Tauri Smoke',
    pipelineLabel: 'Mic + Room',
  });
  await transport.exportTechnicalReport(uploaded.id, { reportId: rep.reportId, format: 'pdf' });
  const daw = await transport.exportDawInterchange(uploaded.id, {
    target: 'audition',
    bitDepth: '24',
    cuesRunId: reg.runId,
    pipelineRunId: pipe.runId,
  });
  assert.equal(daw.target_id, 'audition');

  const mediaBuf = await transport.readMediaBytes('tauri-media://m-11');
  assert.equal(new Uint8Array(mediaBuf)[0], 82);

  await transport.getSettings();
  await transport.saveSettings(DEFAULT_SPECTRACRAFT_SETTINGS);
  await transport.resetSettings();
  const doc = await transport.getHelpDoc('spec');
  assert.equal(doc.id, 'spec');

  // Verify pre-aborted AbortSignal short-circuits before invoking Tauri IPC
  const ac = new AbortController();
  ac.abort();
  await assert.rejects(() => transport.loadReferenceSession(ac.signal), /aborted/i);

  assert.ok(calls.length >= 16);
});
