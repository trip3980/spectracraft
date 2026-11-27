import assert from 'node:assert/strict';
import test from 'node:test';
import {
  DEFAULT_SPECTRACRAFT_SETTINGS,
  DEFAULT_WORKSPACE_PREFS,
  countDirectDepUpdates,
  makeWorkspacePreset,
  pickRecommendedUpdateTarget,
  shouldShowUpdateNotice,
  type UpdateCheckReport,
} from './settingsAndUpdates.ts';

function makeReport(overrides: Partial<UpdateCheckReport> = {}): UpdateCheckReport {
  return {
    checked_at_unix: 1700000000,
    online: true,
    has_updates: true,
    has_staged_download: false,
    staged_target: null,
    latest_notice_id: 'craft:v0.2.0|deps:serde:1.0.230|staged:none',
    summary: 'Newer craft-libs tag v0.2.0 and 1 direct Rust crate release available.',
    craft_libs_repo: 'https://github.com/storytold/craft-libs',
    craft_libs: [
      {
        crate_name: 'craft-pdf',
        wrapper_module: 'spectracraft-mono/src/report_export.rs',
        current_pin_kind: 'rev',
        current_pin: '05b99ee5dcd64f2c954a693ef361301214ddc3c7',
        latest_tag: 'v0.2.0',
        latest_main_rev: 'abcdef1234567890',
        update_available: true,
        incorporated_on_update: true,
      },
      {
        crate_name: 'craft-settings',
        wrapper_module: 'spectracraft-mono/src/settings_store.rs',
        current_pin_kind: 'rev',
        current_pin: '05b99ee5dcd64f2c954a693ef361301214ddc3c7',
        latest_tag: 'v0.2.0',
        latest_main_rev: 'abcdef1234567890',
        update_available: true,
        incorporated_on_update: true,
      },
    ],
    direct_deps: [
      {
        crate_name: 'serde',
        current_version: '1.0.228',
        optional: false,
        latest_version: '1.0.230',
        update_available: true,
        status: 'newer release 1.0.230 available',
      },
      {
        crate_name: 'hound',
        current_version: '3.5.1',
        optional: false,
        latest_version: '3.5.1',
        update_available: false,
        status: 'up to date',
      },
    ],
    shared_artcraft_adopted: ['craft-pdf', 'craft-atomic', 'craft-settings'],
    shared_artcraft_unused: ['craft-wordmodel', 'craft-docx', 'craft-timeline', 'craft-ui'],
    incorporation_policy: 'Pinned shared ArtCraft crates only.',
    ...overrides,
  };
}

test('default settings enable dark-mode report viewer and opt-in launch check', () => {
  assert.equal(DEFAULT_SPECTRACRAFT_SETTINGS.report.dark_mode_viewer, true);
  assert.equal(DEFAULT_SPECTRACRAFT_SETTINGS.report.default_export_format, 'pdf');
  assert.equal(DEFAULT_SPECTRACRAFT_SETTINGS.updates.check_on_launch, true);
  assert.equal(DEFAULT_SPECTRACRAFT_SETTINGS.artcraft.auto_incorporate_shared_crates, true);
  assert.equal(DEFAULT_SPECTRACRAFT_SETTINGS.integration.ai_assist_env_var, 'GEMINI_API_KEY');
});

test('shouldShowUpdateNotice shows on available update and stays hidden after session Not now', () => {
  const report = makeReport();
  assert.equal(shouldShowUpdateNotice(report, DEFAULT_SPECTRACRAFT_SETTINGS, null), true);
  assert.equal(
    shouldShowUpdateNotice(report, DEFAULT_SPECTRACRAFT_SETTINGS, report.latest_notice_id),
    false,
  );
});

test('shouldShowUpdateNotice hides persisted dismissed version until a newer version appears', () => {
  const report = makeReport();
  const dismissedSettings = {
    ...DEFAULT_SPECTRACRAFT_SETTINGS,
    updates: {
      ...DEFAULT_SPECTRACRAFT_SETTINGS.updates,
      last_action: 'not_now' as const,
      dismissed_notice_id: report.latest_notice_id,
    },
  };
  assert.equal(shouldShowUpdateNotice(report, dismissedSettings, null), false);

  const newerReport = makeReport({
    latest_notice_id: 'craft:v0.3.0|deps:serde:1.0.230|staged:none',
  });
  assert.equal(shouldShowUpdateNotice(newerReport, dismissedSettings, null), true);
});

test('shouldShowUpdateNotice surfaces staged downloads waiting for confirmation', () => {
  const staged = makeReport({
    has_updates: true,
    has_staged_download: true,
    staged_target: 'v0.2.0',
    latest_notice_id: 'craft:v0.2.0|deps:none|staged:v0.2.0',
  });
  assert.equal(shouldShowUpdateNotice(staged, DEFAULT_SPECTRACRAFT_SETTINGS, null), true);
  assert.equal(pickRecommendedUpdateTarget(staged), 'v0.2.0');
  assert.equal(shouldShowUpdateNotice(staged, DEFAULT_SPECTRACRAFT_SETTINGS, staged.latest_notice_id), false);
});

test('pickRecommendedUpdateTarget prefers explicit semver tag over main rev and counts direct updates', () => {
  const report = makeReport();
  assert.equal(pickRecommendedUpdateTarget(report), 'v0.2.0');
  assert.equal(countDirectDepUpdates(report), 1);

  const noTagReport = makeReport({
    craft_libs: report.craft_libs.map((e) => ({ ...e, latest_tag: null })),
  });
  assert.equal(pickRecommendedUpdateTarget(noTagReport), 'abcdef1234567890');

  const upToDate = makeReport({
    has_updates: false,
    craft_libs: report.craft_libs.map((e) => ({ ...e, update_available: false })),
    direct_deps: report.direct_deps.map((d) => ({ ...d, update_available: false })),
  });
  assert.equal(shouldShowUpdateNotice(upToDate, DEFAULT_SPECTRACRAFT_SETTINGS, null), false);
  assert.equal(pickRecommendedUpdateTarget(upToDate), null);
});

test('workspace preferences default to a calm ArtCraft layout and expose studio presets', () => {
  assert.equal(DEFAULT_WORKSPACE_PREFS.show_waveform, true);
  assert.equal(DEFAULT_WORKSPACE_PREFS.show_spectrogram, true);
  assert.equal(DEFAULT_WORKSPACE_PREFS.show_tool_rail, true);
  assert.equal(DEFAULT_WORKSPACE_PREFS.show_right_panel, true);
  assert.equal(DEFAULT_WORKSPACE_PREFS.right_panel_tab, 'inspector');
  assert.equal(DEFAULT_WORKSPACE_PREFS.show_room_views, false);
  assert.equal(DEFAULT_WORKSPACE_PREFS.show_diagnostics_dock, false);
  assert.equal(DEFAULT_WORKSPACE_PREFS.show_transport, false);
  assert.equal(DEFAULT_WORKSPACE_PREFS.spatial_mode, 'plans');
  assert.deepEqual(DEFAULT_SPECTRACRAFT_SETTINGS.workspace, DEFAULT_WORKSPACE_PREFS);

  const full = makeWorkspacePreset('full_studio');
  assert.equal(full.show_right_panel, true);
  assert.equal(full.right_panel_tab, 'room');
  assert.equal(full.show_diagnostics_dock, true);
  assert.equal(full.show_tool_rail, true);
  assert.equal(full.show_transport, true);
  assert.equal(full.spatial_mode, 'plans');

  const diag = makeWorkspacePreset('diagnostics');
  assert.equal(diag.show_right_panel, true);
  assert.equal(diag.right_panel_tab, 'diagnostics');
  assert.equal(diag.show_tool_rail, true);
  assert.equal(diag.show_transport, true);

  const reportAudit = makeWorkspacePreset('report_audit');
  assert.equal(reportAudit.show_right_panel, true);
  assert.equal(reportAudit.right_panel_tab, 'report');
  assert.equal(reportAudit.show_waveform, true);
  assert.equal(reportAudit.show_spectrogram, true);
  assert.equal(reportAudit.show_tool_rail, true);

  const focus = makeWorkspacePreset('canvas_focus');
  assert.equal(focus.show_tool_rail, false);
  assert.equal(focus.show_right_panel, false);
  assert.equal(focus.show_waveform, true);
  assert.equal(focus.show_spectrogram, true);

  assert.deepEqual(makeWorkspacePreset('calm'), DEFAULT_WORKSPACE_PREFS);
});

test('M9: pickRecommendedUpdateTarget extracts raw pin from structured StagedTarget, JSON, or legacy display label', () => {
  const fromDetail = makeReport({
    has_staged_download: true,
    staged_target: 'craft-libs@v0.2.5',
    staged_target_detail: { pin: 'v0.2.5', kind: 'tag', display_label: 'craft-libs@v0.2.5' },
  });
  assert.equal(pickRecommendedUpdateTarget(fromDetail), 'v0.2.5');

  const fromLegacy = makeReport({
    has_staged_download: true,
    staged_target: 'craft-libs@05b99ee5',
  });
  assert.equal(pickRecommendedUpdateTarget(fromLegacy), '05b99ee5');

  const fromJson = makeReport({
    has_staged_download: true,
    staged_target: JSON.stringify({ pin: 'v0.3.1', kind: 'tag', display_label: 'craft-libs@v0.3.1' }),
  });
  assert.equal(pickRecommendedUpdateTarget(fromJson), 'v0.3.1');
});



