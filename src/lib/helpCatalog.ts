/**
 * Audited Help catalog, glossary definitions, and direct tool-navigation registry
 * for SpectraCraft. Every entry maps strictly to an implemented feature, stage,
 * panel, diagnostic, or setting in the current codebase.
 */

export type HelpCategory =
  | 'Pipeline Stages'
  | 'Workspace & Panels'
  | 'Diagnostics & Reports'
  | 'Playback & Editing'
  | 'Settings & Updates';

export type HelpNavigationAction =
  | 'open-wav'
  | 'open-reference'
  | 'export-modal'
  | 'stage1-source'
  | 'stage1-shape'
  | 'stage2-regions'
  | 'stage3-mic'
  | 'stage4-room'
  | 'stage4-materials'
  | 'stage5-plugin'
  | 'layers-dock'
  | 'cues-dock'
  | 'repair-dock'
  | 'dry-gate-dock'
  | 'technical-report'
  | 'report-export-pdf'
  | 'workspace-waveform'
  | 'workspace-spectrogram'
  | 'workspace-transport'
  | 'workspace-room-views'
  | 'workspace-tool-rail'
  | 'workspace-right-panel'
  | 'workspace-reset'
  | 'lineage-modal'
  | 'settings-modal'
  | 'settings-appearance'
  | 'settings-workspace'
  | 'settings-updates'
  | 'settings-paths'
  | 'doc-readme'
  | 'doc-spec'
  | 'doc-craft-libs';

export interface HelpTopic {
  id: string;
  title: string;
  category: HelpCategory;
  menuPath: string;
  shortcut?: string;
  summary: string;
  definition: string;
  keywords: string[];
  action: HelpNavigationAction;
  actionLabel: string;
}

export interface EmbeddedDocMeta {
  id: 'readme' | 'spec' | 'craft-libs' | 'design-contract' | 'platform' | 'plugin';
  title: string;
  filename: string;
  description: string;
}

export const EMBEDDED_DOCS: EmbeddedDocMeta[] = [
  {
    id: 'readme',
    title: 'Overview & Five-Stage Pipeline',
    filename: 'README.md',
    description: 'Program overview, five-stage pipeline order, invariants, and CLI commands.',
  },
  {
    id: 'spec',
    title: 'Authoritative Specification',
    filename: 'SPECTRACRAFT_SPEC.md',
    description: 'Canonical Rust architecture, acoustics contract, and verification boundaries.',
  },
  {
    id: 'craft-libs',
    title: 'craft-libs & Update Maintenance Guide',
    filename: 'CRAFT_LIBS_MAINTENANCE.md',
    description: 'Explicit Git pins, thin compatibility wrappers, and deliberate update verification.',
  },
  {
    id: 'design-contract',
    title: 'Design Contract & Dry Listening Gate',
    filename: 'spectracraft-mono/DESIGN-CONTRACT.md',
    description: 'Fresh-signal resynthesis rules, dry listening gate, and visual/audio separation.',
  },
  {
    id: 'platform',
    title: 'Desktop Platform Notes',
    filename: 'PLATFORM.md',
    description: 'Cross-platform Linux, Windows, and macOS desktop workstation notes.',
  },
  {
    id: 'plugin',
    title: 'Engine & Plugin Slot Architecture',
    filename: 'PLUGIN_ARCHITECTURE.md',
    description: 'UI-free buffer-in/buffer-out engine contract for Stage 5 finishing and DAW hosts.',
  },
];

export const HELP_TOPICS: HelpTopic[] = [
  {
    id: 'open-wav',
    title: 'Open Single-Source WAV',
    category: 'Playback & Editing',
    menuPath: 'File → Open Single-Source WAV…',
    shortcut: 'Ctrl+O',
    summary: 'Import one isolated mono WAV (8–96 kHz, up to 90 s) for cue extraction and dry regeneration.',
    definition:
      'SpectraCraft works on one isolated instrument, voice, or stem per session. When opened, the Rust engine extracts immutable performance cues and generates a fresh dry take without copying original waveform samples.',
    keywords: ['import', 'load', 'file', 'wav', 'audio', 'open', 'single source'],
    action: 'open-wav',
    actionLabel: 'Open WAV File…',
  },
  {
    id: 'open-reference',
    title: 'Slow Solo Reference Session (48 kHz)',
    category: 'Playback & Editing',
    menuPath: 'File → Open Slow Solo Reference (48 kHz)',
    summary: 'Load the built-in 48 kHz mono guitar reference session to test the full pipeline.',
    definition:
      'Runs Rust guitar-verify on the bundled 48 kHz reference recording (or deterministic Rust six-note fixture) so you can inspect waveforms, spectrograms, dry regeneration, and before/after reports immediately.',
    keywords: ['reference', 'demo', 'slow solo', 'guitar', 'test wav', 'sample'],
    action: 'open-reference',
    actionLabel: 'Load Reference Session',
  },
  {
    id: 'left-tool-palette',
    title: 'Left Tool Palette (Direct-Manipulation Tools)',
    category: 'Workspace & Panels',
    menuPath: 'Workspace → Left Tool Palette',
    shortcut: 'V / M / R / Z',
    summary: 'ArtCraft/Adobe-style vertical tool strip for Pointer/Scrub, Marquee, Range, Zoom, and canvas display toggles.',
    definition:
      'Docked along the left edge of the workspace. Select Pointer/Scrub (V), Rectangular Marquee (M) for Stage 2 time×frequency dry edits, In/Out Range (R), or Zoom (Z), plus quick Waveform/Spectrogram/Evidence toggles and right-panel tab shortcuts. Can be shown or hidden from the Workspace menu and remembered in config.toml.',
    keywords: ['tool', 'palette', 'left', 'rail', 'pointer', 'scrub', 'marquee', 'range', 'zoom', 'adobe', 'artcraft'],
    action: 'workspace-tool-rail',
    actionLabel: 'Show Left Tool Palette',
  },
  {
    id: 'right-tabbed-panel',
    title: 'Right-Side Tabbed Panel (Inspector, Diagnostics, Report, Room)',
    category: 'Workspace & Panels',
    menuPath: 'Workspace → Right Tabbed Panel',
    summary: 'ArtCraft/Adobe-style tabbed dock on the right keeping contextual tools adjacent to the main audio canvas.',
    definition:
      'Houses four live tabs—Inspector (5-stage pipeline & capture controls), Diagnostics (Dry Listening Gate, Regions, Layers, Cues, Repair, Reference Shape), Report (embedded dark-mode Before/After Technical Report viewer), and Room (interactive 2D Plans & 3D Grid)—so you can switch panels without losing your main waveform/spectrogram canvas.',
    keywords: ['right panel', 'tabs', 'tabbed', 'inspector', 'diagnostics', 'report', 'room', 'dock', 'artcraft', 'adobe'],
    action: 'workspace-right-panel',
    actionLabel: 'Show Right Tabbed Panel',
  },
  {
    id: 'stage-1-dry',
    title: 'Stage 1 — Dry Regeneration & Source Catalog',
    category: 'Pipeline Stages',
    menuPath: 'Process → Stage 1 — Dry Regeneration',
    summary: 'Synthesizes a completely fresh dry performance from extracted cues alone.',
    definition:
      'Stage 1 uses persistent modal string physics (additive re-pick without energy reset, continuous phase pitch bends, and optional body resonance) plus the 138-entry Rust Source Catalog and voicing controls. Zero original PCM ever enters the dry output.',
    keywords: ['stage 1', 'dry', 'regeneration', 'source', 'voicing', 'synthesis', 're-pick', 'bend', 'physics', 'catalog'],
    action: 'stage1-source',
    actionLabel: 'Open Source Inspector',
  },
  {
    id: 'reference-shape',
    title: 'Reference Shape & Structured Synthesis Prompt',
    category: 'Diagnostics & Reports',
    menuPath: 'Right Panel → Diagnostics → Reference Shape',
    summary: 'Immutable evidence summary extracted from the raw input recording.',
    definition:
      'Displays note onsets, pitch range, harmonic distribution, and dynamic contours measured directly from the input file. Profiles and voicing guide resynthesis without overwriting this reference evidence.',
    keywords: ['reference shape', 'evidence', 'prompt', 'pitch', 'onsets', 'contours'],
    action: 'stage1-shape',
    actionLabel: 'Open Reference Shape',
  },
  {
    id: 'stage-2-regions',
    title: 'Stage 2 — Surgical Region Editing (Dry Only)',
    category: 'Pipeline Stages',
    menuPath: 'Process → Stage 2 — Surgical Region Edits',
    shortcut: 'M',
    summary: 'Localized time-range or time×frequency marquee fixes on the cached dry take.',
    definition:
      'Applies targeted repairs strictly to the Stage 1 cached dry take using short crossfades. Audio outside the selected time or frequency region stays bit-identical, and original raw audio is never modified.',
    keywords: ['stage 2', 'region', 'surgical', 'marquee', 'range', 'crossfade', 'edit', 'dry only'],
    action: 'stage2-regions',
    actionLabel: 'Open Region Edits',
  },
  {
    id: 'stage-3-mic',
    title: 'Stage 3 — Optional Microphone',
    category: 'Pipeline Stages',
    menuPath: 'Process → Stage 3 — Optional Microphone',
    summary: 'Optional virtual microphone model, polar pattern, position, and aim (off by default).',
    definition:
      'Models one or more microphones from the factual microphone catalog using exact 343 m/s propagation delay, first-order polar directivity, and distance attenuation. Bypassing Stage 3 preserves settings without blending original audio.',
    keywords: ['stage 3', 'mic', 'microphone', 'polar', 'cardioid', 'sm7b', 'capture', 'bypass'],
    action: 'stage3-mic',
    actionLabel: 'Open Microphone Inspector',
  },
  {
    id: 'stage-4-room',
    title: 'Stage 4 — Optional Space / Room',
    category: 'Pipeline Stages',
    menuPath: 'Process → Stage 4 — Optional Space / Room',
    summary: 'Optional acoustic room geometry, presets, and reflections (off by default).',
    definition:
      'Simulates room dimensions (Small, Medium, Large, Open Field, or Direct), axial room modes, Sabine RT60 decay, and early reflections applied strictly downstream of the dry take.',
    keywords: ['stage 4', 'room', 'space', 'reverb', 'rt60', 'reflections', 'preset', 'acoustics'],
    action: 'stage4-room',
    actionLabel: 'Open Room Inspector',
  },
  {
    id: 'stage-4-materials',
    title: 'Room Surface Materials (6-Band Absorption)',
    category: 'Pipeline Stages',
    menuPath: 'Right Panel → Inspector → Materials',
    summary: 'Per-wall, ceiling, and floor 6-band acoustic absorption profiles.',
    definition:
      'Assigns factual absorption coefficients across six octave bands to each of the six room faces from the surface material catalog, controlling how the room dampens low, mid, and high reflections.',
    keywords: ['materials', 'absorption', 'surfaces', 'walls', 'ceiling', 'floor', 'sabin', '6-band'],
    action: 'stage4-materials',
    actionLabel: 'Open Materials Inspector',
  },
  {
    id: 'stage-5-plugin',
    title: 'Stage 5 — Optional Plugin Slot',
    category: 'Pipeline Stages',
    menuPath: 'Process → Stage 5 — Optional Plugin Slot',
    summary: 'Optional post-capture finishing processors (Gentle EQ, Soft Dynamics, Noise Control, Phase Limiter).',
    definition:
      'Final optional processing slot at the end of the five-stage pipeline. Off by default and independently bypassable without changing upstream dry or room captures.',
    keywords: ['stage 5', 'plugin', 'eq', 'dynamics', 'noise control', 'limiter', 'finishing', 'bypass'],
    action: 'stage5-plugin',
    actionLabel: 'Open Pipeline Controls',
  },
  {
    id: 'technical-report',
    title: 'Before/After Technical Report & Dark-Mode Viewer',
    category: 'Diagnostics & Reports',
    menuPath: 'Right Panel → Report / File → Generate Blind Before/After Technical Report…',
    summary: 'Blind independent acoustic audit comparing raw input against final export with shared-scale plots.',
    definition:
      'Runs two strictly isolated Rust analyses—Before on the raw input only and After on the final export only—then computes significant numeric deltas (hum, DC, true-peak, LUFS, body peaks, room tail). Viewable directly in the Right Panel Report tab (with interactive spectrogram hover readouts, full findings, and one-click Markdown copy) or expanded in full-screen dark mode.',
    keywords: ['report', 'before', 'after', 'blind', 'white paper', 'viewer', 'dark mode', 'spectrogram', 'lufs', 'true peak', 'parseval', 'hover', 'probe'],
    action: 'technical-report',
    actionLabel: 'Open Report Tab',
  },
  {
    id: 'report-export-craft-pdf',
    title: 'Report Export via craft-pdf & craft-atomic (craft-libs)',
    category: 'Diagnostics & Reports',
    menuPath: 'Right Panel → Report → Export PDF / File → Settings & Updates…',
    summary: 'Multi-page PDF generation with embedded plots via pinned craft-pdf and crash-safe craft-atomic writes.',
    definition:
      'Isolated strictly inside spectracraft-mono/src/report_export.rs. Uses pinned craft_pdf::classic::Writer to build dark-mode white-paper PDFs with embedded rasterized plots, and craft_atomic::atomic_write to write exported files via atomic temp-file + rename so disk files are never half-written. Default format (PDF, Markdown, or DOCX fallback) is configured in Settings [report].',
    keywords: ['export', 'pdf', 'craft-pdf', 'craft-atomic', 'craft-libs', 'atomic', 'markdown', 'docx', 'white paper', 'report_export'],
    action: 'report-export-pdf',
    actionLabel: 'Open Report Export',
  },
  {
    id: 'audio-export',
    title: 'Export Dry WAV, Cues & Post-Processing Variants',
    category: 'Playback & Editing',
    menuPath: 'File → Export Dry WAV / Cues / Variants…',
    shortcut: 'Ctrl+E',
    summary: 'Download synthesized dry WAV, cue JSON, and human-approved capture variants.',
    definition:
      'Opens the Export dialog to download the source-free dry take WAV, extracted cues JSON, or batch-rendered capture variants once the 5/5 Dry Listening Gate has passed.',
    keywords: ['export', 'wav', 'download', 'variants', 'cues', 'save'],
    action: 'export-modal',
    actionLabel: 'Open Export Dialog',
  },
  {
    id: 'dry-listening-gate',
    title: 'Dry Listening Gate (5-Criterion Human Review)',
    category: 'Diagnostics & Reports',
    menuPath: 'Right Panel → Diagnostics → Dry Listening Gate',
    summary: 'Explicit 5-point listening check before unlocking batch capture variants.',
    definition:
      'Ensures a human listener verifies pitch accuracy, onset timing, bend continuity, tone, and artifact freedom on the dry take before rendering downstream batch capture variants.',
    keywords: ['listening gate', 'dry review', 'checks', '5/5', 'human', 'approval', 'qa'],
    action: 'dry-gate-dock',
    actionLabel: 'Open Dry Listening Gate',
  },
  {
    id: 'inspection-layers',
    title: 'Editable Inspection Layers (4 Stems)',
    category: 'Diagnostics & Reports',
    menuPath: 'Right Panel → Diagnostics → Editable Layers',
    summary: 'Decomposes a time window into Pitch, Faint Ringing, Texture, and Breath Accent for inspection.',
    definition:
      'Runs Rust analyze-layers and render-layers to let you solo or mute harmonic pitch ridges, resonant body ringing, broadband pick/air texture, and transient accents.',
    keywords: ['layers', 'stems', 'pitch', 'ringing', 'texture', 'breath', 'accent', 'solo', 'mute'],
    action: 'layers-dock',
    actionLabel: 'Open Layers Panel',
  },
  {
    id: 'performance-cues',
    title: 'Performance Cues & Re-Pick Editor',
    category: 'Playback & Editing',
    menuPath: 'Right Panel → Diagnostics → Performance Cues',
    summary: 'Inspect detected note timings, frequencies, level scales, and additive re-pick strikes.',
    definition:
      'Lists every detected note event from cues.json and lets you adjust note level scaling or additive re-pick strikes before re-synthesizing a fresh dry WAV from cues alone.',
    keywords: ['cues', 'notes', 're-pick', 'strike', 'timing', 'level', 'synthesize'],
    action: 'cues-dock',
    actionLabel: 'Open Cues Panel',
  },
  {
    id: 'local-gap-repair',
    title: 'Local Gap Repair',
    category: 'Playback & Editing',
    menuPath: 'Right Panel → Diagnostics → Local Repair',
    summary: 'Repairs a tiny 0.25–4.0 ms dropout interval while keeping all surrounding samples identical.',
    definition:
      'Invokes Rust repair-gap on an explicit millisecond interval with 48 ms edge context. Outside samples are copied bit-identically; unsupported spans are declined honestly.',
    keywords: ['gap', 'repair', 'dropout', 'glitch', 'click', 'interval'],
    action: 'repair-dock',
    actionLabel: 'Open Gap Repair',
  },
  {
    id: 'transport-ab',
    title: 'Transport & Level-Matched A/B Monitoring',
    category: 'Playback & Editing',
    menuPath: 'Workspace → Inline Transport Bar / View → Playback & Take A/B',
    shortcut: 'Space',
    summary: 'Time-aligned playback across Original, Dry Take, Captured, and Difference takes.',
    definition:
      'Play or pause with Space, rewind with Home, and switch seamlessly between Original (1), Dry Take (2), Captured (3), and Difference (4) with optional Rust-measured RMS level matching and selectable preview bit depth (32f / 24b / 16b).',
    keywords: ['play', 'pause', 'transport', 'a/b', 'match db', 'difference', 'bit depth', 'monitoring', 'listen'],
    action: 'workspace-transport',
    actionLabel: 'Show Transport Bar',
  },
  {
    id: 'waveform-spectrogram',
    title: 'Waveform & Spectrogram Views',
    category: 'Workspace & Panels',
    menuPath: 'Workspace → Waveform View / Spectrogram View',
    summary: 'Toggle waveform and spectrogram displays independently without affecting audio output.',
    definition:
      'Purely visual displays for inspecting amplitude envelopes and frequency energy (8–32 kHz, -78 to -112 dB contrast floor, plus visual-only Cue Evidence overlay). Display changes never alter audio or render fingerprints.',
    keywords: ['waveform', 'spectrogram', 'display', 'hz', 'contrast', 'evidence overlay', 'zoom'],
    action: 'workspace-spectrogram',
    actionLabel: 'Show Spectrogram View',
  },
  {
    id: 'room-views-3d',
    title: 'Room Views (2D Plans & 3D Acoustic Grid)',
    category: 'Workspace & Panels',
    menuPath: 'Right Panel → Room / Workspace → Room Views',
    summary: 'Interactive Top/Front floorplans and 3D room view for dragging source and microphone positions.',
    definition:
      'Provides a single shared coordinate space inside the Right Tabbed Panel Room tab for positioning the sound source and virtual microphones in 2D plans, 3D orbit grid, or split mode with optional visual polar patterns and 3:1 distance guide.',
    keywords: ['room views', '3d', 'plans', 'floorplan', 'grid', 'mic position', 'drag', 'spatial'],
    action: 'workspace-room-views',
    actionLabel: 'Open Room Tab',
  },
  {
    id: 'workspace-control',
    title: 'Workspace Layout & Calm Default ([workspace])',
    category: 'Workspace & Panels',
    menuPath: 'Workspace → Reset Workspace / File → Settings & Updates…',
    summary: 'Show or hide Left Tool Palette, Right Tabbed Panel, Waveform, Spectrogram, and Transport; remembered in craft-settings.',
    definition:
      'Starts in a calm ArtCraft default (Left Tool Palette + Audio Canvas + Right Tabbed Panel) and saves every panel toggle, preset (Calm Default, Full Studio, Diagnostics Focus, Report & Blind Audit, Canvas Focus), and active right-panel tab in local uncommitted config.toml via craft-settings. Use Workspace → Reset Workspace at any time to restore defaults.',
    keywords: ['workspace', 'layout', 'panels', 'hide', 'show', 'reset', 'calm', 'presets', 'report audit', 'craft-settings'],
    action: 'settings-workspace',
    actionLabel: 'Open Workspace Settings',
  },
  {
    id: 'lineage-fresh-signal',
    title: 'Resynthesis Lineage & Fresh-Signal Invariant',
    category: 'Diagnostics & Reports',
    menuPath: 'View → Resynthesis Lineage & Scope…',
    summary: 'Documents why original waveform PCM never enters regenerated or captured outputs.',
    definition:
      'Explains SpectraCraft’s strict boundary between immutable input analysis and purely synthesized output samples, plus what is in scope vs out of scope.',
    keywords: ['lineage', 'scope', 'invariant', 'fresh signal', 'no blending', 'original pcm'],
    action: 'lineage-modal',
    actionLabel: 'Open Lineage & Scope',
  },
  {
    id: 'settings-craft-store',
    title: 'craft-settings Storage & Unknown-Key Preservation',
    category: 'Settings & Updates',
    menuPath: 'File → Settings & Updates…',
    shortcut: 'Ctrl+,',
    summary: 'Local versioned ArtCraft config.toml managed by pinned craft-settings (isolated in settings_store.rs).',
    definition:
      'Uses craft_settings::SettingsStore (schema v1, Location::Auto) to store [artcraft], [workspace], [report], [updates], [paths], and [integration] in an uncommitted local config.toml. Unknown ArtCraft suite keys and comments survive SpectraCraft writes intact via toml_edit::DocumentMut.',
    keywords: ['settings', 'preferences', 'config.toml', 'craft-settings', 'craft-libs', 'schema', 'toml', 'reset defaults'],
    action: 'settings-modal',
    actionLabel: 'Open Settings',
  },
  {
    id: 'artcraft-theme-shared',
    title: 'ArtCraft Theme & Shared Crate Adoption ([artcraft])',
    category: 'Settings & Updates',
    menuPath: 'File → Settings & Updates… → Appearance & ArtCraft Defaults',
    summary: 'Controls suite-wide theme preference (dark default / light option) and controlled adoption of pinned craft-libs crates.',
    definition:
      'Stored in the shared [artcraft] table of config.toml. Theme defaults to dark studio mode (matching dark-mode report plots), while auto_incorporate_shared_crates lets accepted updates refresh only already-pinned crates (craft-pdf, craft-atomic, craft-settings) behind thin wrappers without pulling in unused ArtCraft crates.',
    keywords: ['theme', 'dark', 'light', 'artcraft', 'craft-libs', 'shared crates', 'appearance', 'auto_incorporate'],
    action: 'settings-appearance',
    actionLabel: 'Open Appearance Settings',
  },
  {
    id: 'settings-paths-env',
    title: 'Local Paths & Environment Reference ([paths] & [integration])',
    category: 'Settings & Updates',
    menuPath: 'File → Settings & Updates… → Local Paths & Environment Reference',
    summary: 'Local uncommitted default input/export folders and optional AI-assist environment variable name (never raw secrets).',
    definition:
      'Stores local-only last_input_dir and last_export_dir in uncommitted config.toml plus the environment variable name (default GEMINI_API_KEY) inspected on the host. Raw API keys and secrets are never entered in the UI or written to disk.',
    keywords: ['paths', 'folder', 'directory', 'input', 'export', 'env', 'environment', 'gemini_api_key', 'integration'],
    action: 'settings-paths',
    actionLabel: 'Open Paths Settings',
  },
  {
    id: 'update-check-choices',
    title: 'Update Checks & Staged Releases (Settings & Help)',
    category: 'Settings & Updates',
    menuPath: 'Help → Check for Updates & Staged Releases…',
    summary: 'Quiet background launch check for craft-libs and Cargo.toml crates; act on Update now / Download later / Not now from Settings or Help.',
    definition:
      'Runs quietly on startup (when enabled in [updates]) without adding any banner under the top menu bar. Open Help → Check for Updates & Staged Releases… or Settings to inspect pinned craft-libs commits, direct Cargo.toml crates, and staged downloads. "Update now" requires the mandatory 10-second 20 Hz–20 kHz sweep test to pass (rolling back on failure); "Download and update later" fetches only; "Not now" remembers your choice.',
    keywords: ['update', 'updates', 'staged', 'update now', 'download later', 'not now', 'craft-libs', 'pin', 'sweep test', 'rollback'],
    action: 'settings-updates',
    actionLabel: 'Open Updates in Settings',
  },
];

export const HELP_CATEGORIES: HelpCategory[] = [
  'Pipeline Stages',
  'Workspace & Panels',
  'Diagnostics & Reports',
  'Playback & Editing',
  'Settings & Updates',
];

/**
 * Pure type-ahead ranking over all Help topics.
 * Empty query returns all topics in curated order; non-empty query requires every
 * whitespace-separated token to match at least one field and ranks title/keyword hits highest.
 */
export function searchHelpTopics(query: string, topics: readonly HelpTopic[] = HELP_TOPICS): HelpTopic[] {
  const cleaned = query.trim().toLowerCase();
  if (!cleaned) return [...topics];
  const tokens = cleaned.split(/\s+/).filter(Boolean);

  const scored: Array<{ topic: HelpTopic; score: number }> = [];
  for (const topic of topics) {
    const titleL = topic.title.toLowerCase();
    const catL = topic.category.toLowerCase();
    const menuL = topic.menuPath.toLowerCase();
    const sumL = topic.summary.toLowerCase();
    const defL = topic.definition.toLowerCase();
    const kwL = topic.keywords.join(' ').toLowerCase();
    const haystack = `${titleL} ${catL} ${menuL} ${sumL} ${defL} ${kwL}`;

    if (!tokens.every((t) => haystack.includes(t))) continue;

    let score = 0;
    if (titleL.includes(cleaned)) score += 100;
    for (const t of tokens) {
      if (titleL.startsWith(t)) score += 35;
      else if (titleL.includes(t)) score += 25;
      if (topic.keywords.some((k) => k.toLowerCase() === t)) score += 22;
      else if (kwL.includes(t)) score += 14;
      if (menuL.includes(t)) score += 10;
      if (sumL.includes(t)) score += 6;
      if (defL.includes(t)) score += 3;
    }
    scored.push({ topic, score });
  }

  scored.sort((a, b) => b.score - a.score || a.topic.title.localeCompare(b.topic.title));
  return scored.map((s) => s.topic);
}
