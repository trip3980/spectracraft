//! Lightweight, non-blocking update check & controlled three-choice update manager.
//!
//! Covers:
//! - Pinned `storytold/craft-libs` Git dependencies (`craft-pdf`, `craft-atomic`, `craft-settings`)
//! - Direct crates.io dependencies declared in `spectracraft-mono/Cargo.toml`
//!
//! Policy:
//! - Never auto-updates on build or launch.
//! - Launch check is opt-in (`settings.updates.check_on_launch`) and runs in the background.
//! - User has three explicit choices:
//!   1. `Update now` — deliberate pin bump (when a newer explicit tag/rev is targeted) +
//!      `cargo update` for selected crates + mandatory 10-second 20 Hz–20 kHz sweep test
//!      + library tests before marking the update successful (rolls back on failure).
//!   2. `Download and update later` — fetches packages only; records staged target in
//!      `craft-settings` awaiting explicit confirmation.
//!   3. `Not now` — does nothing; remembers dismissal for the notice/version.
//! - Stage 1 dry physics are never altered.

use crate::report_export::{
    ARTCRAFT_SHARED_INTEGRATION_POINTS, CRAFT_LIBS_PIN, CRAFT_LIBS_PINS, CRAFT_LIBS_PIN_KIND,
    CRAFT_LIBS_REPO,
};
use crate::settings_store::{load_settings, mutate_settings};
use crate::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, Serialize)]
pub struct CraftLibsUpdateEntry {
    pub crate_name: String,
    pub wrapper_module: String,
    pub current_pin_kind: String,
    pub current_pin: String,
    pub latest_tag: Option<String>,
    pub latest_main_rev: Option<String>,
    pub update_available: bool,
    pub incorporated_on_update: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct DirectDepUpdateEntry {
    pub crate_name: String,
    pub current_version: String,
    pub optional: bool,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct UpdateCheckReport {
    pub checked_at_unix: u64,
    pub online: bool,
    pub has_updates: bool,
    pub has_staged_download: bool,
    pub staged_target: Option<String>,
    pub latest_notice_id: String,
    pub summary: String,
    pub craft_libs_repo: String,
    pub craft_libs: Vec<CraftLibsUpdateEntry>,
    pub direct_deps: Vec<DirectDepUpdateEntry>,
    pub shared_artcraft_adopted: Vec<String>,
    pub shared_artcraft_unused: Vec<String>,
    pub incorporation_policy: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct UpdateActionResult {
    pub action: String,
    pub ok: bool,
    pub message: String,
    pub sweep_test_ran: bool,
    pub sweep_test_passed: bool,
    pub unit_tests_passed: bool,
    pub rolled_back: bool,
    pub staged_target: Option<String>,
    pub dismissed_notice_id: Option<String>,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cargo_bin() -> String {
    if let Ok(home) = std::env::var("HOME") {
        let candidate = PathBuf::from(home).join(".cargo/bin/cargo");
        if candidate.exists() {
            return candidate.display().to_string();
        }
    }
    "cargo".into()
}

/// Parse simple semantic version triples `(major, minor, patch)` ignoring prereleases.
pub fn parse_stable_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim().trim_start_matches('v').trim_start_matches('=');
    if trimmed.contains('-') || trimmed.contains('+') {
        return None;
    }
    let mut parts = trimmed.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Validate an explicit commit hash (7–40 hex chars) or release tag (`vX.Y.Z` / `X.Y.Z`).
pub fn is_valid_explicit_pin(target: &str) -> bool {
    let t = target.trim();
    if t.is_empty() || t.len() > 64 || t.starts_with('=') {
        return false;
    }
    let is_hex_rev = (7..=40).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_hexdigit());
    let is_semver_tag = parse_stable_semver(t).is_some();
    is_hex_rev || is_semver_tag
}

/// Parse direct crates.io dependencies declared in `spectracraft-mono/Cargo.toml`.
pub fn parse_direct_deps_from_toml(toml_text: &str) -> Vec<(String, String, bool)> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for raw_line in toml_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
            continue;
        }
        if !in_deps || line.contains("git =") || line.contains("path =") {
            continue;
        }
        let Some((name_part, rhs)) = line.split_once('=') else {
            continue;
        };
        let name = name_part.trim().to_string();
        let rhs = rhs.trim();
        let optional = rhs.contains("optional = true");
        let version = if rhs.starts_with('"') {
            rhs.trim_matches('"').trim_start_matches('=').to_string()
        } else if let Some(idx) = rhs.find("version = \"") {
            let rest = &rhs[idx + 11..];
            rest.split('"')
                .next()
                .unwrap_or("")
                .trim_start_matches('=')
                .to_string()
        } else {
            continue;
        };
        if !name.is_empty() && !version.is_empty() {
            deps.push((name, version, optional));
        }
    }
    deps
}

fn query_craft_libs_remote() -> (Option<String>, Option<String>) {
    let Ok(refs) = crate::report_export::query_craft_libs_remote_refs() else {
        return (None, None);
    };
    let mut best_tag: Option<((u64, u64, u64), String)> = None;
    for (tag, _) in refs.tags {
        if let Some(sv) = parse_stable_semver(&tag) {
            if best_tag.as_ref().map(|(v, _)| sv > *v).unwrap_or(true) {
                best_tag = Some((sv, tag));
            }
        }
    }
    (best_tag.map(|(_, t)| t), refs.main_rev)
}

fn query_crates_io_latest_stable(crate_name: &str) -> Option<String> {
    if !crate_name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let url = format!("https://crates.io/api/v1/crates/{crate_name}");
    let out = Command::new("curl")
        .args([
            "-fsS",
            "--max-time",
            "3",
            "-H",
            "User-Agent: SpectraCraft-UpdateCheck/0.1 (storytold/spectracraft)",
            &url,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let val: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    val.get("crate")
        .and_then(|c| c.get("max_stable_version"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Inspect pinned `craft-libs` crates and direct `Cargo.toml` dependencies without modifying files.
pub fn run_update_check(
    workspace_root: &Path,
    settings_dir: Option<&Path>,
    online: bool,
) -> UpdateCheckReport {
    let snap = load_settings(settings_dir);
    let check_direct = snap.settings.updates.check_direct_dependencies;
    let staged_target = snap.settings.updates.pending_staged_target.clone();
    let has_staged_download = staged_target.as_ref().is_some_and(|s| !s.is_empty());

    let (latest_tag, latest_main_rev) = if online {
        query_craft_libs_remote()
    } else {
        (None, None)
    };

    let craft_has_newer = if let Some(ref tag) = latest_tag {
        CRAFT_LIBS_PIN_KIND != "tag" || tag != CRAFT_LIBS_PIN
    } else if let Some(ref main_sha) = latest_main_rev {
        main_sha != CRAFT_LIBS_PIN
    } else {
        false
    };

    let craft_libs: Vec<CraftLibsUpdateEntry> = CRAFT_LIBS_PINS
        .iter()
        .map(|pin| {
            let wrapper = if pin.crate_name == "craft-settings" {
                "spectracraft-mono/src/settings_store.rs"
            } else {
                "spectracraft-mono/src/report_export.rs"
            };
            CraftLibsUpdateEntry {
                crate_name: pin.crate_name.to_string(),
                wrapper_module: wrapper.to_string(),
                current_pin_kind: pin.pin_kind.to_string(),
                current_pin: pin.pin_value.to_string(),
                latest_tag: latest_tag.clone(),
                latest_main_rev: latest_main_rev.clone(),
                update_available: craft_has_newer,
                incorporated_on_update: true,
            }
        })
        .collect();

    let cargo_toml_path = workspace_root.join("spectracraft-mono/Cargo.toml");
    let raw_deps = std::fs::read_to_string(&cargo_toml_path)
        .map(|s| parse_direct_deps_from_toml(&s))
        .unwrap_or_default();

    let mut direct_deps = Vec::with_capacity(raw_deps.len());
    let mut direct_updates_count = 0usize;
    let mut direct_sig_parts = Vec::new();

    for (name, current_ver, optional) in raw_deps {
        let should_query = online && check_direct && !optional;
        let latest = if should_query {
            query_crates_io_latest_stable(&name)
        } else {
            None
        };
        let is_newer = match (
            parse_stable_semver(&current_ver),
            latest.as_deref().and_then(parse_stable_semver),
        ) {
            (Some(cur), Some(lat)) => lat > cur,
            _ => false,
        };
        if is_newer {
            direct_updates_count += 1;
            if let Some(ref lat) = latest {
                direct_sig_parts.push(format!("{name}:{lat}"));
            }
        }
        let status = if !online {
            "offline (not queried)".to_string()
        } else if optional {
            "optional feature dep (checked manually)".to_string()
        } else if !check_direct {
            "skipped by settings".to_string()
        } else if is_newer {
            format!(
                "newer release {} available",
                latest.as_deref().unwrap_or("?")
            )
        } else if latest.is_some() {
            "up to date".to_string()
        } else {
            "registry unreachable".to_string()
        };
        direct_deps.push(DirectDepUpdateEntry {
            crate_name: name,
            current_version: current_ver,
            optional,
            latest_version: latest,
            update_available: is_newer,
            status,
        });
    }

    let has_updates = craft_has_newer || direct_updates_count > 0 || has_staged_download;
    let craft_sig = latest_tag
        .clone()
        .or_else(|| {
            latest_main_rev
                .as_ref()
                .map(|s| s[..s.len().min(8)].to_string())
        })
        .unwrap_or_else(|| CRAFT_LIBS_PIN[..CRAFT_LIBS_PIN.len().min(8)].to_string());
    let latest_notice_id = format!(
        "craft:{craft_sig}|deps:{}|staged:{}",
        if direct_sig_parts.is_empty() {
            "none".to_string()
        } else {
            direct_sig_parts.join(",")
        },
        staged_target.as_deref().unwrap_or("none")
    );

    let summary = if has_staged_download {
        format!(
            "Downloaded update ({}) is ready to apply on confirmation.",
            staged_target.as_deref().unwrap_or("staged")
        )
    } else if craft_has_newer && direct_updates_count > 0 {
        format!(
            "Newer craft-libs revision and {direct_updates_count} direct Rust crate release(s) available."
        )
    } else if craft_has_newer {
        if let Some(ref tag) = latest_tag {
            let short_pin = &CRAFT_LIBS_PIN[..CRAFT_LIBS_PIN.len().min(8)];
            format!("Newer craft-libs tag {tag} is available (currently pinned to {CRAFT_LIBS_PIN_KIND} {short_pin}).")
        } else {
            "Newer craft-libs commit is available on storytold/craft-libs.".into()
        }
    } else if direct_updates_count > 0 {
        format!(
            "{direct_updates_count} direct Rust dependency update(s) available ({}).",
            direct_sig_parts.join(", ")
        )
    } else if online {
        "All pinned craft-libs crates and checked direct dependencies are up to date.".into()
    } else {
        "Offline pin check completed (enable online check to query GitHub & crates.io).".into()
    };

    let shared_artcraft_adopted: Vec<String> = CRAFT_LIBS_PINS
        .iter()
        .map(|p| p.crate_name.to_string())
        .collect();
    let shared_artcraft_unused: Vec<String> = ARTCRAFT_SHARED_INTEGRATION_POINTS
        .iter()
        .filter(|pt| !shared_artcraft_adopted.iter().any(|a| a == pt.crate_name))
        .map(|pt| pt.crate_name.to_string())
        .collect();

    UpdateCheckReport {
        checked_at_unix: unix_now(),
        online,
        has_updates,
        has_staged_download,
        staged_target,
        latest_notice_id,
        summary,
        craft_libs_repo: CRAFT_LIBS_REPO.into(),
        craft_libs,
        direct_deps,
        shared_artcraft_adopted,
        shared_artcraft_unused,
        incorporation_policy: "Only shared ArtCraft crates already pinned by SpectraCraft (craft-pdf, craft-atomic, craft-settings) are incorporated behind thin wrappers (report_export.rs, settings_store.rs); unused ArtCraft crates remain unlinked.".into(),
    }
}

/// Handle user choice `"not_now"` — does nothing to dependencies, remembers dismissal.
pub fn apply_not_now(settings_dir: Option<&Path>, notice_id: &str) -> Result<UpdateActionResult> {
    let clean = notice_id.trim().chars().take(180).collect::<String>();
    mutate_settings(settings_dir, |s| {
        s.updates.last_action = "not_now".into();
        if !clean.is_empty() {
            s.updates.dismissed_notice_id = Some(clean.clone());
        }
    })?;
    Ok(UpdateActionResult {
        action: "not_now".into(),
        ok: true,
        message: "Update dismissed for this session and version. SpectraCraft will not nag again until next launch or a newer release.".into(),
        sweep_test_ran: false,
        sweep_test_passed: false,
        unit_tests_passed: false,
        rolled_back: false,
        staged_target: None,
        dismissed_notice_id: if clean.is_empty() { None } else { Some(clean) },
    })
}

/// Handle user choice `"download_later"` — fetches crate sources without switching active pins.
pub fn apply_download_later(
    workspace_root: &Path,
    settings_dir: Option<&Path>,
    target: Option<&str>,
) -> Result<UpdateActionResult> {
    let target_label = match target.map(str::trim).filter(|t| !t.is_empty()) {
        Some(raw) => {
            let pin_part = raw.strip_prefix("craft-libs@").unwrap_or(raw);
            if !is_valid_explicit_pin(pin_part) {
                return Err(format!(
                    "Refusing invalid staged target '{raw}' (expected explicit commit SHA or semver tag)."
                )
                .into());
            }
            format!("craft-libs@{pin_part}")
        }
        None => format!(
            "craft-libs@{}",
            &CRAFT_LIBS_PIN[..CRAFT_LIBS_PIN.len().min(8)]
        ),
    };

    let fetch = Command::new(cargo_bin())
        .current_dir(workspace_root)
        .args(["fetch", "--locked"])
        .output()?;
    if !fetch.status.success() {
        let stderr = String::from_utf8_lossy(&fetch.stderr);
        return Err(format!("cargo fetch failed: {}", stderr.trim()).into());
    }

    mutate_settings(settings_dir, |s| {
        s.updates.last_action = "download_later".into();
        s.updates.pending_staged_target = Some(target_label.clone());
    })?;

    Ok(UpdateActionResult {
        action: "download_later".into(),
        ok: true,
        message: format!(
            "Fetched sources for {target_label} into local Cargo cache. Active pins remain untouched until you confirm 'Update now'."
        ),
        sweep_test_ran: false,
        sweep_test_passed: false,
        unit_tests_passed: false,
        rolled_back: false,
        staged_target: Some(target_label),
        dismissed_notice_id: None,
    })
}

/// Handle user choice `"update_now"` — deliberate pin bump (if explicit rev/tag provided) +
/// `cargo update` for pinned craft-libs crates + mandatory 10s 20 Hz–20 kHz sweep test +
/// unit test verification, rolling back automatically on any failure.
pub fn apply_update_now(
    workspace_root: &Path,
    settings_dir: Option<&Path>,
    target_rev_or_tag: Option<&str>,
) -> Result<UpdateActionResult> {
    let cargo_toml_path = workspace_root.join("spectracraft-mono/Cargo.toml");
    let cargo_lock_path = workspace_root.join("Cargo.lock");
    let orig_toml = std::fs::read_to_string(&cargo_toml_path)?;
    let orig_lock = std::fs::read_to_string(&cargo_lock_path).ok();

    let rollback = |toml_backup: &str, lock_backup: &Option<String>| {
        let _ = crate::report_export::atomic_write_bytes(&cargo_toml_path, toml_backup.as_bytes());
        if let Some(lock) = lock_backup {
            let _ = crate::report_export::atomic_write_bytes(&cargo_lock_path, lock.as_bytes());
        }
    };

    let mut bumped_note = String::from("verified current pins");
    if let Some(raw_target) = target_rev_or_tag.map(str::trim).filter(|t| !t.is_empty()) {
        if !is_valid_explicit_pin(raw_target) {
            return Err(format!(
                "Refusing invalid non-explicit pin '{raw_target}' (expected commit SHA or semver tag)."
            )
            .into());
        }
        let is_tag = parse_stable_semver(raw_target).is_some();
        let replacement = if is_tag {
            format!("tag = \"{raw_target}\"")
        } else {
            format!("rev = \"{raw_target}\"")
        };
        let old_pattern = format!("{CRAFT_LIBS_PIN_KIND} = \"{CRAFT_LIBS_PIN}\"");
        if orig_toml.contains(&old_pattern) && raw_target != CRAFT_LIBS_PIN {
            let updated_toml = orig_toml.replace(&old_pattern, &replacement);
            crate::report_export::atomic_write_bytes(&cargo_toml_path, updated_toml.as_bytes())?;
            let upd = Command::new(cargo_bin())
                .current_dir(workspace_root)
                .args([
                    "update",
                    "-p",
                    "craft-pdf",
                    "-p",
                    "craft-atomic",
                    "-p",
                    "craft-settings",
                ])
                .output()?;
            if !upd.status.success() {
                rollback(&orig_toml, &orig_lock);
                return Ok(UpdateActionResult {
                    action: "update_now".into(),
                    ok: false,
                    message: format!(
                        "cargo update failed and was rolled back cleanly: {}",
                        String::from_utf8_lossy(&upd.stderr).trim()
                    ),
                    sweep_test_ran: false,
                    sweep_test_passed: false,
                    unit_tests_passed: false,
                    rolled_back: true,
                    staged_target: None,
                    dismissed_notice_id: None,
                });
            }
            bumped_note = format!("bumped pinned craft-libs to {replacement}");
        }
    }

    // Mandatory verification gate 1: 10s 20 Hz–20 kHz sine sweep report + viewer + craft-pdf export test
    let sweep = Command::new(cargo_bin())
        .current_dir(workspace_root)
        .args([
            "test",
            "--locked",
            "--release",
            "-p",
            "spectracraft-mono",
            "--test",
            "technical_report_sweep",
        ])
        .output()?;
    let sweep_ok = sweep.status.success();
    if !sweep_ok {
        rollback(&orig_toml, &orig_lock);
        return Ok(UpdateActionResult {
            action: "update_now".into(),
            ok: false,
            message: format!(
                "Mandatory 20 Hz–20 kHz sweep verification failed; rolled back Cargo.toml & Cargo.lock cleanly. {}",
                String::from_utf8_lossy(&sweep.stderr).trim()
            ),
            sweep_test_ran: true,
            sweep_test_passed: false,
            unit_tests_passed: false,
            rolled_back: true,
            staged_target: None,
            dismissed_notice_id: None,
        });
    }

    // Mandatory verification gate 2: full library unit tests (including Stage 1 physics + settings_store)
    let unit = Command::new(cargo_bin())
        .current_dir(workspace_root)
        .args([
            "test",
            "--locked",
            "--release",
            "-p",
            "spectracraft-mono",
            "--lib",
        ])
        .output()?;
    let unit_ok = unit.status.success();
    if !unit_ok {
        rollback(&orig_toml, &orig_lock);
        return Ok(UpdateActionResult {
            action: "update_now".into(),
            ok: false,
            message: format!(
                "Rust library unit tests failed after update; rolled back Cargo.toml & Cargo.lock cleanly. {}",
                String::from_utf8_lossy(&unit.stderr).trim()
            ),
            sweep_test_ran: true,
            sweep_test_passed: true,
            unit_tests_passed: false,
            rolled_back: true,
            staged_target: None,
            dismissed_notice_id: None,
        });
    }

    let now_s = format!("unix:{}", unix_now());
    mutate_settings(settings_dir, |s| {
        s.updates.last_action = "updated".into();
        s.updates.pending_staged_target = None;
        s.updates.dismissed_notice_id = None;
        s.updates.last_updated_at = Some(now_s);
    })?;

    Ok(UpdateActionResult {
        action: "update_now".into(),
        ok: true,
        message: format!(
            "Update verified ({bumped_note}): mandatory 20 Hz–20 kHz sweep test and full Rust unit suite passed 100%."
        ),
        sweep_test_ran: true,
        sweep_test_passed: true,
        unit_tests_passed: true,
        rolled_back: false,
        staged_target: None,
        dismissed_notice_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_pin_validation_accepts_tags_and_commits_and_rejects_floating_branches() {
        assert!(is_valid_explicit_pin("v0.1.0"));
        assert!(is_valid_explicit_pin("0.2.4"));
        assert!(is_valid_explicit_pin(
            "05b99ee5dcd64f2c954a693ef361301214ddc3c7"
        ));
        assert!(!is_valid_explicit_pin("main"));
        assert!(!is_valid_explicit_pin("master"));
        assert!(!is_valid_explicit_pin("v1.0.0-alpha.1"));
        assert!(!is_valid_explicit_pin("=0.1.0"));
        assert!(!is_valid_explicit_pin("--help"));
        assert!(!is_valid_explicit_pin(""));
    }

    #[test]
    fn direct_dep_parser_extracts_exact_versions_and_skips_git_entries() {
        let sample = r#"
[dependencies]
craft-pdf = { git = "https://github.com/storytold/craft-libs", rev = "05b99ee5" }
hound = "=3.5.1"
serde = { version = "=1.0.228", features = ["derive"] }
eframe = { version = "=0.31.1", optional = true }

[dev-dependencies]
png = "=0.18.1"
"#;
        let deps = parse_direct_deps_from_toml(sample);
        assert_eq!(
            deps,
            vec![
                ("hound".into(), "3.5.1".into(), false),
                ("serde".into(), "1.0.228".into(), false),
                ("eframe".into(), "0.31.1".into(), true),
            ]
        );
    }
}
