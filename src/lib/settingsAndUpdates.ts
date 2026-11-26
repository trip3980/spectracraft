export interface ArtCraftSharedPrefs {
  theme: 'dark' | 'light';
  auto_incorporate_shared_crates: boolean;
}

export interface UpdatePrefs {
  check_on_launch: boolean;
  check_direct_dependencies: boolean;
  last_action: 'none' | 'not_now' | 'download_later' | 'updated';
  dismissed_notice_id: string | null;
  pending_staged_target: string | null;
  last_updated_at: string | null;
}

export interface ReportPrefs {
  dark_mode_viewer: boolean;
  default_export_format: 'pdf' | 'markdown' | 'docx';
  auto_open_viewer: boolean;
}

export interface PathPrefs {
  last_input_dir: string;
  last_export_dir: string;
}

export interface IntegrationPrefs {
  ai_assist_env_var: string;
}

export type RightPanelTab = 'inspector' | 'diagnostics' | 'report' | 'room';

export interface WorkspacePrefs {
  show_waveform: boolean;
  show_spectrogram: boolean;
  show_inspector: boolean;
  show_right_panel: boolean;
  right_panel_tab: RightPanelTab;
  show_room_views: boolean;
  show_diagnostics_dock: boolean;
  show_tool_rail: boolean;
  show_transport: boolean;
  spatial_mode: 'plans' | '3d' | 'split';
}

export interface SpectraCraftSettings {
  artcraft: ArtCraftSharedPrefs;
  updates: UpdatePrefs;
  report: ReportPrefs;
  workspace: WorkspacePrefs;
  paths: PathPrefs;
  integration: IntegrationPrefs;
}

export interface SettingsSnapshot {
  app_id: string;
  schema_version: number;
  backend_crate: string;
  config_path: string | null;
  location_rule: string;
  load_source: string;
  read_only: boolean;
  ai_assist_key_present: boolean;
  settings: SpectraCraftSettings;
}

export interface CraftLibsUpdateEntry {
  crate_name: string;
  wrapper_module: string;
  current_pin_kind: string;
  current_pin: string;
  latest_tag: string | null;
  latest_main_rev: string | null;
  update_available: boolean;
  incorporated_on_update: boolean;
}

export interface DirectDepUpdateEntry {
  crate_name: string;
  current_version: string;
  optional: boolean;
  latest_version: string | null;
  update_available: boolean;
  status: string;
}

export interface UpdateCheckReport {
  checked_at_unix: number;
  online: boolean;
  has_updates: boolean;
  has_staged_download: boolean;
  staged_target: string | null;
  latest_notice_id: string;
  summary: string;
  craft_libs_repo: string;
  craft_libs: CraftLibsUpdateEntry[];
  direct_deps: DirectDepUpdateEntry[];
  shared_artcraft_adopted: string[];
  shared_artcraft_unused: string[];
  incorporation_policy: string;
}

export type UpdateActionKind = 'not_now' | 'download_later' | 'update_now';

export interface UpdateActionResult {
  action: UpdateActionKind | string;
  ok: boolean;
  message: string;
  sweep_test_ran: boolean;
  sweep_test_passed: boolean;
  unit_tests_passed: boolean;
  rolled_back: boolean;
  staged_target: string | null;
  dismissed_notice_id: string | null;
}

export const DEFAULT_WORKSPACE_PREFS: WorkspacePrefs = {
  show_waveform: true,
  show_spectrogram: true,
  show_inspector: true,
  show_right_panel: true,
  right_panel_tab: 'inspector',
  show_room_views: false,
  show_diagnostics_dock: false,
  show_tool_rail: true,
  show_transport: false,
  spatial_mode: 'plans',
};

export type WorkspacePresetKey =
  | 'calm'
  | 'full_studio'
  | 'diagnostics'
  | 'report_audit'
  | 'canvas_focus';

export function makeWorkspacePreset(preset: WorkspacePresetKey): WorkspacePrefs {
  if (preset === 'full_studio') {
    return {
      show_waveform: true,
      show_spectrogram: true,
      show_inspector: true,
      show_right_panel: true,
      right_panel_tab: 'room',
      show_room_views: true,
      show_diagnostics_dock: true,
      show_tool_rail: true,
      show_transport: true,
      spatial_mode: 'plans',
    };
  }
  if (preset === 'diagnostics') {
    return {
      show_waveform: true,
      show_spectrogram: true,
      show_inspector: true,
      show_right_panel: true,
      right_panel_tab: 'diagnostics',
      show_room_views: false,
      show_diagnostics_dock: false,
      show_tool_rail: true,
      show_transport: true,
      spatial_mode: 'plans',
    };
  }
  if (preset === 'report_audit') {
    return {
      show_waveform: true,
      show_spectrogram: true,
      show_inspector: true,
      show_right_panel: true,
      right_panel_tab: 'report',
      show_room_views: false,
      show_diagnostics_dock: false,
      show_tool_rail: true,
      show_transport: true,
      spatial_mode: 'plans',
    };
  }
  if (preset === 'canvas_focus') {
    return {
      show_waveform: true,
      show_spectrogram: true,
      show_inspector: false,
      show_right_panel: false,
      right_panel_tab: 'inspector',
      show_room_views: false,
      show_diagnostics_dock: false,
      show_tool_rail: false,
      show_transport: false,
      spatial_mode: 'plans',
    };
  }
  return { ...DEFAULT_WORKSPACE_PREFS };
}

export const DEFAULT_SPECTRACRAFT_SETTINGS: SpectraCraftSettings = {
  artcraft: {
    theme: 'dark',
    auto_incorporate_shared_crates: true,
  },
  updates: {
    check_on_launch: true,
    check_direct_dependencies: true,
    last_action: 'none',
    dismissed_notice_id: null,
    pending_staged_target: null,
    last_updated_at: null,
  },
  report: {
    dark_mode_viewer: true,
    default_export_format: 'pdf',
    auto_open_viewer: true,
  },
  workspace: { ...DEFAULT_WORKSPACE_PREFS },
  paths: {
    last_input_dir: '',
    last_export_dir: '',
  },
  integration: {
    ai_assist_env_var: 'GEMINI_API_KEY',
  },
};

/**
 * Determine whether the non-modal launch update banner should be displayed.
 *
 * Rules:
 * - Never shows if there are neither updates nor a previously downloaded staged target.
 * - Remembers the user's choice for the current session (`sessionDismissedId`).
 * - Respects a persisted `Not now` dismissal for the same notice fingerprint until a
 *   newer version appears (unless a staged download awaits explicit confirmation).
 */
export function shouldShowUpdateNotice(
  report: UpdateCheckReport | null,
  settings: SpectraCraftSettings | null,
  sessionDismissedId: string | null,
): boolean {
  if (!report) return false;
  if (!report.has_updates && !report.has_staged_download) return false;
  if (sessionDismissedId && sessionDismissedId === report.latest_notice_id) {
    return false;
  }
  const persistedDismissed = settings?.updates.dismissed_notice_id ?? null;
  if (
    !report.has_staged_download &&
    persistedDismissed &&
    persistedDismissed === report.latest_notice_id
  ) {
    return false;
  }
  return true;
}

/**
 * Pick an explicit tag or commit revision target for `craft-libs` when a newer
 * upstream release is detected; returns `null` when `craft-libs` is already current.
 */
export function pickRecommendedUpdateTarget(report: UpdateCheckReport | null): string | null {
  if (!report) return null;
  if (report.staged_target && report.staged_target.trim().length > 0) {
    return report.staged_target.trim();
  }
  const newerCraft = report.craft_libs.find((entry) => entry.update_available);
  if (!newerCraft) return null;
  if (newerCraft.latest_tag && newerCraft.latest_tag.trim().length > 0) {
    return newerCraft.latest_tag.trim();
  }
  if (newerCraft.latest_main_rev && newerCraft.latest_main_rev.trim().length > 0) {
    return newerCraft.latest_main_rev.trim();
  }
  return null;
}

export function countDirectDepUpdates(report: UpdateCheckReport | null): number {
  if (!report) return 0;
  return report.direct_deps.filter((d) => d.update_available).length;
}
