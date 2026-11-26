import assert from 'node:assert/strict';
import test from 'node:test';
import { EMBEDDED_DOCS, HELP_TOPICS, searchHelpTopics } from './helpCatalog.ts';

test('help catalog covers all five pipeline stages, ArtCraft panels, craft-libs crates, and updates', () => {
  const ids = new Set(HELP_TOPICS.map((t) => t.id));
  for (const required of [
    'left-tool-palette',
    'right-tabbed-panel',
    'stage-1-dry',
    'stage-2-regions',
    'stage-3-mic',
    'stage-4-room',
    'stage-5-plugin',
    'technical-report',
    'report-export-craft-pdf',
    'workspace-control',
    'settings-craft-store',
    'artcraft-theme-shared',
    'settings-paths-env',
    'update-check-choices',
  ]) {
    assert.ok(ids.has(required), `Missing required Help topic: ${required}`);
  }
  assert.ok(EMBEDDED_DOCS.some((d) => d.id === 'readme'));
  assert.ok(EMBEDDED_DOCS.some((d) => d.id === 'craft-libs'));
});

test('searchHelpTopics returns ranked matches for craft-libs, panels, and tool queries', () => {
  const reportHits = searchHelpTopics('before after report');
  assert.ok(reportHits.length >= 1);
  assert.equal(reportHits[0]?.id, 'technical-report');
  assert.equal(reportHits[0]?.action, 'technical-report');

  const pdfHits = searchHelpTopics('craft-pdf');
  assert.ok(pdfHits.length >= 1);
  assert.equal(pdfHits[0]?.id, 'report-export-craft-pdf');

  const atomicHits = searchHelpTopics('craft-atomic');
  assert.ok(atomicHits.length >= 1);
  assert.equal(atomicHits[0]?.id, 'report-export-craft-pdf');

  const settingsHits = searchHelpTopics('craft-settings');
  assert.ok(settingsHits.length >= 1);
  assert.equal(settingsHits[0]?.id, 'settings-craft-store');
  assert.equal(settingsHits[0]?.action, 'settings-modal');

  const themeHits = searchHelpTopics('artcraft theme');
  assert.ok(themeHits.length >= 1);
  assert.equal(themeHits[0]?.id, 'artcraft-theme-shared');
  assert.equal(themeHits[0]?.action, 'settings-appearance');

  const toolHits = searchHelpTopics('left tool palette');
  assert.ok(toolHits.length >= 1);
  assert.equal(toolHits[0]?.id, 'left-tool-palette');
  assert.equal(toolHits[0]?.action, 'workspace-tool-rail');

  const rightPanelHits = searchHelpTopics('right tabbed panel');
  assert.ok(rightPanelHits.length >= 1);
  assert.equal(rightPanelHits[0]?.id, 'right-tabbed-panel');
  assert.equal(rightPanelHits[0]?.action, 'workspace-right-panel');

  const updateHits = searchHelpTopics('staged not now');
  assert.ok(updateHits.length >= 1);
  assert.equal(updateHits[0]?.id, 'update-check-choices');
  assert.equal(updateHits[0]?.action, 'settings-updates');

  const micHits = searchHelpTopics('microphone polar');
  assert.ok(micHits.length >= 1);
  assert.equal(micHits[0]?.id, 'stage-3-mic');

  const noHits = searchHelpTopics('zzzz-nonexistent-feature-xyz');
  assert.equal(noHits.length, 0);
});

test('empty help query returns full curated list without mutating catalog', () => {
  const all = searchHelpTopics('   ');
  assert.equal(all.length, HELP_TOPICS.length);
  all.pop();
  assert.equal(searchHelpTopics('').length, HELP_TOPICS.length);
});
