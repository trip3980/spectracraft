//! Right-hand Five-Stage Pipeline Inspector & Review Gate for the experimental GPUI frontend.
//!
//! Renders the canonical SpectraCraft sequence without hiding stage semantics:
//!   Stage 1: Dry Regeneration (source profile + voicing)
//!   Review Gate: Five-criterion revision-bound dry listening checklist (`SC-GOV-04`)
//!   Stage 2: Surgical Region Editing (dry only, bit-identical outside region, actual-audio Undo/Redo)
//!   Stage 3: Optional Microphone (independent bypass + model + preamp drive)
//!   Stage 4: Optional Space / Room (independent bypass + room preset)
//!   Stage 5: Optional Plugin Slot (independent bypass + processor selection)
//!   Export & Preset: Explicit Dry vs Current Capture atomic WAV export + honest Settings Preset JSON

use crate::app::SpectraCraftGpuiApp;
use gpui::{Context, IntoElement, SharedString, div, prelude::*, px, rgb};
use spectracraft_mono::app_session::ExportTarget;
use spectracraft_mono::capture::DryReviewStatus;
use spectracraft_mono::pipeline::PluginId;
use spectracraft_mono::region_edit::RegionFixAction;
use spectracraft_mono::reperform::CharacterTarget;
use spectracraft_mono::studio_catalog::RoomPreset;

fn section_card(title: &str, badge: Option<(&str, u32)>) -> gpui::Div {
    let mut header = div()
        .flex()
        .items_center()
        .justify_between()
        .mb(px(6.0))
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(0xeeeef3))
                .child(title.to_owned()),
        );
    if let Some((label, color_hex)) = badge {
        header = header.child(
            div()
                .px(px(6.0))
                .py(px(1.0))
                .rounded(px(3.0))
                .bg(rgb(0x1b1c22))
                .border_1()
                .border_color(rgb(color_hex))
                .text_xs()
                .text_color(rgb(color_hex))
                .child(label.to_owned()),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .p(px(8.0))
        .rounded(px(4.0))
        .bg(rgb(0x26262c))
        .border_1()
        .border_color(rgb(0x3a3a44))
        .child(header)
}

fn action_btn(
    id: &'static str,
    label: impl Into<SharedString>,
    active: bool,
    disabled: bool,
    cx: &mut Context<SpectraCraftGpuiApp>,
    on_click: impl Fn(&mut SpectraCraftGpuiApp, &mut Context<SpectraCraftGpuiApp>) + 'static,
) -> impl IntoElement {
    let bg = if disabled {
        rgb(0x232329)
    } else if active {
        rgb(0x2a5c96)
    } else {
        rgb(0x2e2e36)
    };
    let fg = if disabled {
        rgb(0x6c6d78)
    } else {
        rgb(0xeeeef3)
    };
    let border = if active { rgb(0x378ef0) } else { rgb(0x3a3a44) };

    let mut el = div()
        .id(id)
        .px(px(8.0))
        .py(px(4.0))
        .rounded(px(3.0))
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_xs()
        .text_color(fg)
        .child(label.into());
    if !disabled {
        el = el
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0x3a3a45)).border_color(rgb(0x378ef0)))
            .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)));
    }
    el
}

pub fn render_inspector(
    app: &SpectraCraftGpuiApp,
    cx: &mut Context<SpectraCraftGpuiApp>,
) -> impl IntoElement {
    let has_source = app.session.source.is_some();
    let has_dry = app.session.dry.is_some();
    let has_capture = app.session.capture.is_some();
    let capture_stale = app.session.capture_stale;
    let source_name = app.session.current_source_entry().name.clone();
    let dry_sha_short = app
        .session
        .dry
        .as_ref()
        .map(|d| d.sha256.chars().take(12).collect::<String>())
        .unwrap_or_else(|| "none".to_owned());
    let cap_sha_short = app
        .session
        .capture
        .as_ref()
        .map(|c| c.sha256.chars().take(12).collect::<String>())
        .unwrap_or_else(|| "none".to_owned());
    let review_status = app.session.dry_review_status();
    let checklist = app.session.dry_review.checklist;

    // Stage 1 card
    let stage1 = section_card(
        "Stage 1 - Dry Regeneration",
        Some(if has_dry {
            ("Dry Ready", 0x34d399)
        } else {
            ("Pending", 0x9ca3af)
        }),
    )
    .child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(4.0))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0xa1a1ac))
                    .child(format!("Source: {source_name}")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(4.0))
                    .child(action_btn("src-prev", "<", false, false, cx, |this, cx| {
                        this.step_source_index(-1, cx);
                    }))
                    .child(action_btn("src-next", ">", false, false, cx, |this, cx| {
                        this.step_source_index(1, cx);
                    })),
            ),
    )
    .child(
        div()
            .flex()
            .flex_wrap()
            .gap(px(4.0))
            .child(action_btn(
                "char-keep",
                "Keep",
                app.session.voicing.character_target == CharacterTarget::Keep,
                false,
                cx,
                |this, cx| this.set_character_target(CharacterTarget::Keep, cx),
            ))
            .child(action_btn(
                "char-natural",
                "Natural",
                app.session.voicing.character_target == CharacterTarget::Natural,
                false,
                cx,
                |this, cx| this.set_character_target(CharacterTarget::Natural, cx),
            ))
            .child(action_btn(
                "char-analog",
                "Analog",
                app.session.voicing.character_target == CharacterTarget::Analog,
                false,
                cx,
                |this, cx| this.set_character_target(CharacterTarget::Analog, cx),
            ))
            .child(action_btn(
                "char-digital",
                "Digital",
                app.session.voicing.character_target == CharacterTarget::Digital,
                false,
                cx,
                |this, cx| this.set_character_target(CharacterTarget::Digital, cx),
            )),
    )
    .child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(action_btn(
                "run-stage1",
                "Regenerate Dry",
                false,
                !has_source,
                cx,
                |this, cx| this.trigger_dry_regeneration(cx),
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0xa1a1ac))
                    .child(format!("SHA: {dry_sha_short}")),
            ),
    );

    // Dry Listening Review Gate (SC-GOV-04)
    let review_badge = if review_status == DryReviewStatus::ApprovedByHuman {
        ("Candidate Reviewed", 0x34d399)
    } else {
        ("Not Reviewed", 0xf59e0b)
    };
    let review_card = section_card("Dry Listening Review (SC-GOV-04)", Some(review_badge))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.0))
                .child(action_btn(
                    "chk-rhythm",
                    if checklist.rhythm {
                        "[x] Rhythm"
                    } else {
                        "[ ] Rhythm"
                    },
                    checklist.rhythm,
                    !has_dry,
                    cx,
                    |this, cx| this.toggle_checklist_item(0, cx),
                ))
                .child(action_btn(
                    "chk-phrasing",
                    if checklist.phrasing {
                        "[x] Phrasing"
                    } else {
                        "[ ] Phrasing"
                    },
                    checklist.phrasing,
                    !has_dry,
                    cx,
                    |this, cx| this.toggle_checklist_item(1, cx),
                ))
                .child(action_btn(
                    "chk-dynamics",
                    if checklist.dynamics {
                        "[x] Dynamics"
                    } else {
                        "[ ] Dynamics"
                    },
                    checklist.dynamics,
                    !has_dry,
                    cx,
                    |this, cx| this.toggle_checklist_item(2, cx),
                ))
                .child(action_btn(
                    "chk-sustain",
                    if checklist.sustain {
                        "[x] Sustain"
                    } else {
                        "[ ] Sustain"
                    },
                    checklist.sustain,
                    !has_dry,
                    cx,
                    |this, cx| this.toggle_checklist_item(3, cx),
                ))
                .child(action_btn(
                    "chk-artifact",
                    if checklist.artifact_free {
                        "[x] Artifact-Free"
                    } else {
                        "[ ] Artifact-Free"
                    },
                    checklist.artifact_free,
                    !has_dry,
                    cx,
                    |this, cx| this.toggle_checklist_item(4, cx),
                )),
        )
        .child(div().text_xs().text_color(rgb(0xa1a1ac)).child(format!(
            "{}/5 criteria bound to dry SHA {dry_sha_short}",
            checklist.passed_count()
        )));

    // Stage 2 card (Dry only)
    let sel_text = match app.session.selection {
        Some([a, b]) => format!("Region: {a:.3}s - {b:.3}s"),
        None => "Region: drag canvas or click Auto-Select".to_owned(),
    };
    let draft_action = app.session.region_edit_draft.action;
    let stage2 = section_card("Stage 2 - Surgical Edit (Dry Only)", None)
        .child(div().text_xs().text_color(rgb(0xa1a1ac)).child(sel_text))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.0))
                .child(action_btn(
                    "act-stabilize",
                    "Stabilize",
                    draft_action == RegionFixAction::Stabilize,
                    !has_dry,
                    cx,
                    |this, cx| this.set_region_action(RegionFixAction::Stabilize, cx),
                ))
                .child(action_btn(
                    "act-smooth",
                    "Smooth",
                    draft_action == RegionFixAction::Smooth,
                    !has_dry,
                    cx,
                    |this, cx| this.set_region_action(RegionFixAction::Smooth, cx),
                ))
                .child(action_btn(
                    "act-straighten",
                    "Straighten",
                    draft_action == RegionFixAction::Straighten,
                    !has_dry,
                    cx,
                    |this, cx| this.set_region_action(RegionFixAction::Straighten, cx),
                ))
                .child(action_btn(
                    "act-noise",
                    "Reduce Noise",
                    draft_action == RegionFixAction::ReduceNoise,
                    !has_dry,
                    cx,
                    |this, cx| this.set_region_action(RegionFixAction::ReduceNoise, cx),
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(action_btn(
                    "sel-middle",
                    "Select Mid 25%",
                    false,
                    !has_dry,
                    cx,
                    |this, cx| this.select_middle_quarter(cx),
                ))
                .child(action_btn(
                    "apply-stage2",
                    "Apply Dry Edit",
                    false,
                    !has_dry || app.session.selection.is_none(),
                    cx,
                    |this, cx| this.trigger_region_edit(cx),
                )),
        )
        .child(
            div()
                .flex()
                .gap(px(4.0))
                .child(action_btn(
                    "btn-undo",
                    format!("Undo ({})", app.session.history.past_len()),
                    false,
                    !app.session.history.can_undo(),
                    cx,
                    |this, cx| this.perform_undo(cx),
                ))
                .child(action_btn(
                    "btn-redo",
                    format!("Redo ({})", app.session.history.future_len()),
                    false,
                    !app.session.history.can_redo(),
                    cx,
                    |this, cx| this.perform_redo(cx),
                )),
        );

    // Stages 3-5 card (Independent Bypass + Preamp Drive + Room + Plugin)
    let mic_idx = app
        .session
        .rig
        .mics
        .get(app.session.rig.selected_mic)
        .map(|m| m.catalog_index)
        .unwrap_or(0);
    let mic_name = app
        .session
        .mics
        .get(mic_idx)
        .map(|m| format!("{} {}", m.brand, m.model))
        .unwrap_or_else(|| "Default Mic".to_owned());
    let capture_badge = if capture_stale {
        ("Stale Capture", 0xf59e0b)
    } else if has_capture {
        ("Current", 0x34d399)
    } else {
        ("Bypass / Idle", 0x9ca3af)
    };
    let plugin_id = app.session.plugin_slot.plugin_id;
    let stage345 = section_card("Stages 3-5 - Post-Dry Capture", Some(capture_badge))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.0))
                .child(action_btn(
                    "tog-mic",
                    if app.session.mic_enabled {
                        "Stage 3 Mic: ON"
                    } else {
                        "Stage 3 Mic: OFF"
                    },
                    app.session.mic_enabled,
                    false,
                    cx,
                    |this, cx| this.toggle_mic_stage(cx),
                ))
                .child(action_btn(
                    "tog-room",
                    if app.session.room_enabled {
                        "Stage 4 Room: ON"
                    } else {
                        "Stage 4 Room: OFF"
                    },
                    app.session.room_enabled,
                    false,
                    cx,
                    |this, cx| this.toggle_room_stage(cx),
                ))
                .child(action_btn(
                    "tog-plugin",
                    if app.session.plugin_slot.enabled {
                        "Stage 5 Plugin: ON"
                    } else {
                        "Stage 5 Plugin: OFF"
                    },
                    app.session.plugin_slot.enabled,
                    false,
                    cx,
                    |this, cx| this.toggle_plugin_stage(cx),
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xa1a1ac))
                        .child(format!("Mic: {mic_name}")),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(4.0))
                        .child(action_btn("mic-prev", "<", false, false, cx, |this, cx| {
                            this.step_mic_index(-1, cx);
                        }))
                        .child(action_btn("mic-next", ">", false, false, cx, |this, cx| {
                            this.step_mic_index(1, cx);
                        })),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_xs().text_color(rgb(0xa1a1ac)).child(format!(
                    "Preamp Drive: {:.0}%",
                    app.session.preamp_drive * 100.0
                )))
                .child(
                    div()
                        .flex()
                        .gap(px(4.0))
                        .child(action_btn(
                            "preamp-down",
                            "-",
                            false,
                            false,
                            cx,
                            |this, cx| {
                                this.step_preamp_drive(-0.1, cx);
                            },
                        ))
                        .child(action_btn(
                            "preamp-up",
                            "+",
                            false,
                            false,
                            cx,
                            |this, cx| {
                                this.step_preamp_drive(0.1, cx);
                            },
                        )),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xa1a1ac))
                        .child(format!("Room: {}", app.session.rig.room_preset.label())),
                )
                .child(action_btn(
                    "room-cycle",
                    "Cycle Room",
                    false,
                    false,
                    cx,
                    |this, cx| {
                        let next = match this.session.rig.room_preset {
                            RoomPreset::Direct => RoomPreset::Small,
                            RoomPreset::Small => RoomPreset::Medium,
                            RoomPreset::Medium => RoomPreset::Large,
                            RoomPreset::Large => RoomPreset::Open,
                            RoomPreset::Open => RoomPreset::Direct,
                        };
                        this.set_room_preset(next, cx);
                    },
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xa1a1ac))
                        .child(format!("Plugin: {plugin_id:?}")),
                )
                .child(action_btn(
                    "plugin-cycle",
                    "Cycle Plugin",
                    false,
                    false,
                    cx,
                    |this, cx| {
                        let next = match this.session.plugin_slot.plugin_id {
                            PluginId::None => PluginId::GentleEq,
                            PluginId::GentleEq => PluginId::SoftDynamics,
                            PluginId::SoftDynamics => PluginId::NoiseControl,
                            PluginId::NoiseControl => PluginId::PhaseLimiter,
                            PluginId::PhaseLimiter => PluginId::None,
                        };
                        this.set_plugin_id(next, cx);
                    },
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(action_btn(
                    "run-post-dry",
                    "Render Stages 3-5",
                    false,
                    !has_dry,
                    cx,
                    |this, cx| this.trigger_post_dry_render(cx),
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xa1a1ac))
                        .child(format!("SHA: {cap_sha_short}")),
                ),
        );

    // Export & Settings Preset card
    let export_card = section_card("Atomic Export & Settings Preset", None)
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.0))
                .child(action_btn(
                    "export-dry",
                    "Export Dry WAV",
                    false,
                    !has_dry,
                    cx,
                    |this, cx| this.trigger_export_dialog(ExportTarget::Dry, cx),
                ))
                .child(action_btn(
                    "export-cap",
                    if capture_stale {
                        "Capture Stale (Re-Render)"
                    } else {
                        "Export Current Capture"
                    },
                    false,
                    !has_capture || capture_stale,
                    cx,
                    |this, cx| this.trigger_export_dialog(ExportTarget::CurrentCapture, cx),
                )),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.0))
                .child(action_btn(
                    "export-report",
                    "Blind Report (PDF/MD)",
                    false,
                    !has_dry,
                    cx,
                    |this, cx| this.trigger_export_report_dialog(cx),
                ))
                .child(action_btn(
                    "export-daw",
                    "DAW Bundle",
                    false,
                    !has_dry,
                    cx,
                    |this, cx| this.trigger_export_daw_dialog(cx),
                )),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.0))
                .child(action_btn(
                    "save-preset",
                    "Save Settings Preset",
                    false,
                    false,
                    cx,
                    |this, cx| this.trigger_save_preset_dialog(cx),
                ))
                .child(action_btn(
                    "load-preset",
                    "Load Settings Preset",
                    false,
                    false,
                    cx,
                    |this, cx| this.trigger_load_preset_dialog(cx),
                )),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(0x8e8f9c))
                .child("Presets sync seamlessly with Tauri (iOS/Android/Desktop). WAV & reports use craft-atomic."),
        );

    div()
        .w(px(336.0))
        .h_full()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .p(px(10.0))
        .bg(rgb(0x1f1f24))
        .border_l_1()
        .border_color(rgb(0x3a3a44))
        .child(stage1)
        .child(review_card)
        .child(stage2)
        .child(stage345)
        .child(export_card)
}
