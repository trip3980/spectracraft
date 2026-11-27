//! Lightweight, non-blocking update check & controlled three-choice update manager.
//!
//! Covers:
//! - Pinned `storytold/craft-libs` Git dependencies (`craft-pdf`, `craft-atomic`, `craft-settings`)
//! - Direct crates.io dependencies declared in `spectracraft-mono/Cargo.toml`
//!
//! Transactional & Staging Policy (Audit H12, M9):
//! - Never auto-updates on build or launch.
//! - Serializes all mutating update/staging operations behind an exclusive `.spectracraft-update.lock`
//!   file (`create_new(true)`) with stale dead-PID detection and automatic RAII cleanup.
//! - Stages `Cargo.toml` and `Cargo.lock` into an isolated temporary workspace BEFORE running
//!   `cargo update`, `cargo fetch`, or verification tests. Live workspace manifests are only
//!   published atomically AFTER all verification gates pass.
//! - Runs every Cargo subprocess with an explicit timeout and kills the child process on timeout.
//! - Verifies restored SHA-256 bytes before ever reporting `rolled_back: true`.
//! - Persists structured `StagedTarget { pin, kind, display_label }` for `"download_later"` so
//!   subsequent `"update_now"` consumes the exact selected target without pin-validation failure.
//! - Stage 1 dry physics are never altered.

use crate::digest::sha256;
use crate::report_export::{
    ARTCRAFT_SHARED_INTEGRATION_POINTS, CRAFT_LIBS_PIN, CRAFT_LIBS_PINS, CRAFT_LIBS_PIN_KIND,
    CRAFT_LIBS_REPO,
};
use crate::settings_store::{load_settings, mutate_settings};
use crate::Result;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const UPDATE_LOCK_FILE: &str = ".spectracraft-update.lock";
const UPDATE_CMD_TIMEOUT: Duration = Duration::from_secs(600);

/// Structured staged update target (Audit M9) separating raw pin from display formatting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedTarget {
    pub pin: String,
    pub kind: String,
    pub display_label: String,
}

impl StagedTarget {
    /// Parse a raw pin (`v0.2.0` or commit SHA), legacy `"craft-libs@<pin>"`, or JSON `StagedTarget`.
    pub fn from_raw(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("Staged target cannot be empty.".into());
        }
        if trimmed.starts_with('{') {
            if let Ok(parsed) = serde_json::from_str::<StagedTarget>(trimmed) {
                return Self::from_pin(&parsed.pin);
            }
        }
        let pin_part = trimmed.strip_prefix("craft-libs@").unwrap_or(trimmed).trim();
        Self::from_pin(pin_part)
    }

    pub fn from_pin(pin: &str) -> Result<Self> {
        let clean = pin.trim();
        if !is_valid_explicit_pin(clean) {
            return Err(format!(
                "Refusing invalid explicit target '{clean}' (expected commit SHA or semver tag)."
            )
            .into());
        }
        let kind = if parse_stable_semver(clean).is_some() {
            "tag".to_string()
        } else {
            "rev".to_string()
        };
        Ok(Self {
            pin: clean.to_string(),
            kind,
            display_label: format!("craft-libs@{clean}"),
        })
    }

    pub fn toml_pin_assignment(&self) -> String {
        format!("{} = \"{}\"", self.kind, self.pin)
    }

    pub fn to_persisted_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| self.pin.clone())
    }
}

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
    pub staged_target_detail: Option<StagedTarget>,
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
    pub rollback_verified: bool,
    pub staged_target: Option<String>,
    pub staged_target_detail: Option<StagedTarget>,
    pub dismissed_notice_id: Option<String>,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cargo_bin() -> String {
    if let Ok(explicit) = std::env::var("SPECTRACRAFT_CARGO_BIN") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
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

#[derive(Debug, Deserialize, Serialize)]
struct UpdateLockPayload {
    pid: u32,
    created_at_unix: u64,
}

fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    if pid == std::process::id() {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(true)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// RAII exclusive update lock preventing concurrent manifest/lockfile mutations (Audit H12).
struct UpdateLockGuard {
    lock_path: PathBuf,
    pid: u32,
}

impl UpdateLockGuard {
    fn acquire(workspace_root: &Path) -> Result<Self> {
        Self::acquire_with_liveness(workspace_root, std::process::id(), is_pid_alive)
    }

    fn acquire_with_liveness(
        workspace_root: &Path,
        pid: u32,
        check_alive: fn(u32) -> bool,
    ) -> Result<Self> {
        let lock_path = workspace_root.join(UPDATE_LOCK_FILE);
        let payload = UpdateLockPayload {
            pid,
            created_at_unix: unix_now(),
        };
        let encoded = serde_json::to_vec(&payload)?;

        let try_create = |path: &Path| -> std::io::Result<()> {
            let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
            f.write_all(&encoded)?;
            f.sync_all()?;
            Ok(())
        };

        match try_create(&lock_path) {
            Ok(()) => Ok(Self { lock_path, pid }),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = std::fs::read_to_string(&lock_path)
                    .ok()
                    .and_then(|raw| serde_json::from_str::<UpdateLockPayload>(&raw).ok())
                    .map(|existing| existing.pid != pid && !check_alive(existing.pid))
                    .unwrap_or(false);
                if stale {
                    let _ = std::fs::remove_file(&lock_path);
                    try_create(&lock_path).map_err(|_| {
                        "Another SpectraCraft update process acquired the update lock concurrently."
                    })?;
                    return Ok(Self { lock_path, pid });
                }
                Err("Another SpectraCraft dependency update is already in progress.".into())
            }
            Err(err) => Err(format!("Failed to acquire update lock: {err}").into()),
        }
    }
}

impl Drop for UpdateLockGuard {
    fn drop(&mut self) {
        if let Ok(raw) = std::fs::read_to_string(&self.lock_path) {
            if let Ok(parsed) = serde_json::from_str::<UpdateLockPayload>(&raw) {
                if parsed.pid == self.pid {
                    let _ = std::fs::remove_file(&self.lock_path);
                }
            }
        }
    }
}

#[derive(Debug)]
struct TimedCommandOutput {
    success: bool,
    timed_out: bool,
    stderr: String,
}

/// Spawn a subprocess with an explicit timeout and guaranteed kill + reap on timeout (Audit H12).
fn run_command_with_timeout(
    program: &str,
    args: &[&str],
    cwd: &Path,
    extra_env: &[(&str, &Path)],
    timeout: Duration,
) -> Result<TimedCommandOutput> {
    let mut cmd = Command::new(program);
    cmd.current_dir(cwd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    let mut child = cmd
        .spawn()
        .map_err(|err| format!("Failed to start '{program}': {err}"))?;
    let mut stderr_pipe = child.stderr.take();
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut pipe) = stderr_pipe {
            let _ = pipe.take(64 * 1024).read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).trim().to_string()
    });

    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };

    let stderr = stderr_handle.join().unwrap_or_default();
    Ok(TimedCommandOutput {
        success: status.map(|s| s.success()).unwrap_or(false),
        timed_out,
        stderr,
    })
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ty.is_file() {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn link_or_copy_entry(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !src.exists() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(src, dst).is_ok() {
            return Ok(());
        }
    }
    if src.is_dir() {
        copy_dir_recursive(src, dst)
    } else {
        std::fs::copy(src, dst)?;
        Ok(())
    }
}

/// Isolated temporary Cargo staging workspace so failed resolution/tests never mutate live manifests.
struct StagedWorkspace {
    root: PathBuf,
    cargo_toml_path: PathBuf,
    cargo_lock_path: PathBuf,
}

impl StagedWorkspace {
    fn create(workspace_root: &Path, patched_crate_toml: &str, orig_lock: Option<&[u8]>) -> Result<Self> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "spectracraft-update-stage-{}-{stamp}",
            std::process::id()
        ));
        let crate_dir = root.join("spectracraft-mono");
        std::fs::create_dir_all(&crate_dir)?;

        let root_toml_src = workspace_root.join("Cargo.toml");
        if root_toml_src.exists() {
            std::fs::copy(&root_toml_src, root.join("Cargo.toml"))?;
        }
        let cargo_lock_path = root.join("Cargo.lock");
        if let Some(lock_bytes) = orig_lock {
            std::fs::write(&cargo_lock_path, lock_bytes)?;
        }

        let cargo_toml_path = crate_dir.join("Cargo.toml");
        std::fs::write(&cargo_toml_path, patched_crate_toml.as_bytes())?;

        let src_crate_dir = workspace_root.join("spectracraft-mono");
        for item in ["src", "tests", "benches", "assets", "data", "build.rs"] {
            let from = src_crate_dir.join(item);
            let to = crate_dir.join(item);
            link_or_copy_entry(&from, &to)?;
        }

        Ok(Self {
            root,
            cargo_toml_path,
            cargo_lock_path,
        })
    }
}

impl Drop for StagedWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Restore pre-update bytes and verify SHA-256 matches original snapshot BEFORE reporting rollback success.
fn restore_and_verify_snapshot(
    cargo_toml_path: &Path,
    orig_toml_bytes: &[u8],
    cargo_lock_path: &Path,
    orig_lock_bytes: Option<&[u8]>,
) -> bool {
    let expected_toml_sha = sha256(orig_toml_bytes);
    let _ = crate::report_export::atomic_write_bytes(cargo_toml_path, orig_toml_bytes);
    let toml_ok = std::fs::read(cargo_toml_path)
        .map(|b| sha256(&b) == expected_toml_sha)
        .unwrap_or(false);

    let lock_ok = match orig_lock_bytes {
        Some(orig_lock) => {
            let expected_lock_sha = sha256(orig_lock);
            let _ = crate::report_export::atomic_write_bytes(cargo_lock_path, orig_lock);
            std::fs::read(cargo_lock_path)
                .map(|b| sha256(&b) == expected_lock_sha)
                .unwrap_or(false)
        }
        None => true,
    };

    toml_ok && lock_ok
}

fn patch_craft_libs_pin(orig_toml: &str, target: &StagedTarget) -> (String, bool) {
    let old_pattern = format!("{CRAFT_LIBS_PIN_KIND} = \"{CRAFT_LIBS_PIN}\"");
    let replacement = target.toml_pin_assignment();
    if orig_toml.contains(&old_pattern) && target.pin != CRAFT_LIBS_PIN {
        (orig_toml.replace(&old_pattern, &replacement), true)
    } else {
        (orig_toml.to_string(), false)
    }
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
    let staged_target_detail = snap
        .settings
        .updates
        .pending_staged_target
        .as_deref()
        .and_then(|raw| StagedTarget::from_raw(raw).ok());
    let staged_target = staged_target_detail.as_ref().map(|s| s.pin.clone());
    let has_staged_download = staged_target_detail.is_some();

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

    let summary = if let Some(ref detail) = staged_target_detail {
        format!(
            "Downloaded update ({}) is ready to apply on confirmation.",
            detail.display_label
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
        staged_target_detail,
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
        rollback_verified: false,
        staged_target: None,
        staged_target_detail: None,
        dismissed_notice_id: if clean.is_empty() { None } else { Some(clean) },
    })
}

/// Handle user choice `"download_later"` (Audit H12, M9):
/// Acquires exclusive update lock, resolves and fetches the selected target's lockfile and
/// sources inside an isolated temp staging workspace without touching live manifests, and
/// persists a structured `StagedTarget`.
pub fn apply_download_later(
    workspace_root: &Path,
    settings_dir: Option<&Path>,
    target: Option<&str>,
) -> Result<UpdateActionResult> {
    let _lock = UpdateLockGuard::acquire(workspace_root)?;
    let staged = match target.map(str::trim).filter(|t| !t.is_empty()) {
        Some(raw) => StagedTarget::from_raw(raw)?,
        None => StagedTarget::from_pin(CRAFT_LIBS_PIN)?,
    };

    let cargo_toml_path = workspace_root.join("spectracraft-mono/Cargo.toml");
    let cargo_lock_path = workspace_root.join("Cargo.lock");
    let orig_toml = std::fs::read_to_string(&cargo_toml_path)?;
    let orig_lock = std::fs::read(&cargo_lock_path).ok();
    let (patched_toml, changed) = patch_craft_libs_pin(&orig_toml, &staged);

    let stage = StagedWorkspace::create(workspace_root, &patched_toml, orig_lock.as_deref())?;
    let target_dir = workspace_root.join("target");
    let cargo = cargo_bin();

    if changed {
        let upd = run_command_with_timeout(
            &cargo,
            &[
                "update",
                "-p",
                "craft-pdf",
                "-p",
                "craft-atomic",
                "-p",
                "craft-settings",
            ],
            &stage.root,
            &[("CARGO_TARGET_DIR", target_dir.as_path())],
            UPDATE_CMD_TIMEOUT,
        )?;
        if !upd.success {
            let reason = if upd.timed_out {
                "timed out while resolving staged lockfile".to_string()
            } else {
                upd.stderr
            };
            return Err(format!("cargo update in staging failed: {reason}").into());
        }
    }

    let fetch = run_command_with_timeout(
        &cargo,
        &["fetch", "--locked"],
        &stage.root,
        &[("CARGO_TARGET_DIR", target_dir.as_path())],
        UPDATE_CMD_TIMEOUT,
    )?;
    if !fetch.success {
        let reason = if fetch.timed_out {
            "timed out while fetching selected crate sources".to_string()
        } else {
            fetch.stderr
        };
        return Err(format!("cargo fetch in staging failed: {reason}").into());
    }

    mutate_settings(settings_dir, |s| {
        s.updates.last_action = "download_later".into();
        s.updates.pending_staged_target = Some(staged.to_persisted_json());
    })?;

    Ok(UpdateActionResult {
        action: "download_later".into(),
        ok: true,
        message: format!(
            "Resolved and fetched sources for {} into local Cargo cache via isolated staging. Live manifests remain untouched until you confirm 'Update now'.",
            staged.display_label
        ),
        sweep_test_ran: false,
        sweep_test_passed: false,
        unit_tests_passed: false,
        rolled_back: false,
        rollback_verified: false,
        staged_target: Some(staged.pin.clone()),
        staged_target_detail: Some(staged),
        dismissed_notice_id: None,
    })
}

/// Handle user choice `"update_now"` (Audit H12, M9):
/// Acquires exclusive lock, stages candidate manifests into an isolated temporary workspace,
/// runs `cargo update` + mandatory 20 Hz–20 kHz sweep test + Rust library unit tests with
/// explicit timeouts, and publishes manifests to the live workspace ONLY after all gates pass.
pub fn apply_update_now(
    workspace_root: &Path,
    settings_dir: Option<&Path>,
    target_rev_or_tag: Option<&str>,
) -> Result<UpdateActionResult> {
    let _lock = UpdateLockGuard::acquire(workspace_root)?;

    let cargo_toml_path = workspace_root.join("spectracraft-mono/Cargo.toml");
    let cargo_lock_path = workspace_root.join("Cargo.lock");
    let orig_toml = std::fs::read_to_string(&cargo_toml_path)?;
    let orig_toml_bytes = orig_toml.as_bytes().to_vec();
    let orig_lock_bytes = std::fs::read(&cargo_lock_path).ok();

    let resolved_target: Option<StagedTarget> = match target_rev_or_tag
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        Some(raw) => Some(StagedTarget::from_raw(raw)?),
        None => load_settings(settings_dir)
            .settings
            .updates
            .pending_staged_target
            .as_deref()
            .and_then(|raw| StagedTarget::from_raw(raw).ok()),
    };

    let (candidate_toml, pin_changed, bumped_note) = match resolved_target.as_ref() {
        Some(target) => {
            let (patched, changed) = patch_craft_libs_pin(&orig_toml, target);
            let note = if changed {
                format!("bumped pinned craft-libs to {}", target.toml_pin_assignment())
            } else {
                format!("verified pin {}", target.display_label)
            };
            (patched, changed, note)
        }
        None => (orig_toml.clone(), false, "verified current pins".to_string()),
    };

    let stage =
        StagedWorkspace::create(workspace_root, &candidate_toml, orig_lock_bytes.as_deref())?;
    let target_dir = workspace_root.join("target");
    let cargo = cargo_bin();

    if pin_changed {
        let upd = match run_command_with_timeout(
            &cargo,
            &[
                "update",
                "-p",
                "craft-pdf",
                "-p",
                "craft-atomic",
                "-p",
                "craft-settings",
            ],
            &stage.root,
            &[("CARGO_TARGET_DIR", target_dir.as_path())],
            UPDATE_CMD_TIMEOUT,
        ) {
            Ok(out) => out,
            Err(err) => {
                return Ok(UpdateActionResult {
                    action: "update_now".into(),
                    ok: false,
                    message: format!(
                        "Could not run cargo update in staging ({err}); live workspace was not modified."
                    ),
                    sweep_test_ran: false,
                    sweep_test_passed: false,
                    unit_tests_passed: false,
                    rolled_back: false,
                    rollback_verified: true,
                    staged_target: resolved_target.as_ref().map(|s| s.pin.clone()),
                    staged_target_detail: resolved_target,
                    dismissed_notice_id: None,
                });
            }
        };
        if !upd.success {
            let reason = if upd.timed_out {
                "timed out and subprocess was terminated".to_string()
            } else {
                upd.stderr
            };
            return Ok(UpdateActionResult {
                action: "update_now".into(),
                ok: false,
                message: format!(
                    "Staged cargo update failed ({reason}); live workspace was not modified."
                ),
                sweep_test_ran: false,
                sweep_test_passed: false,
                unit_tests_passed: false,
                rolled_back: false,
                rollback_verified: true,
                staged_target: resolved_target.as_ref().map(|s| s.pin.clone()),
                staged_target_detail: resolved_target,
                dismissed_notice_id: None,
            });
        }
    }

    // Mandatory verification gate 1: 10s 20 Hz–20 kHz sine sweep report + viewer + craft-pdf export test
    let sweep = match run_command_with_timeout(
        &cargo,
        &[
            "test",
            "--locked",
            "--release",
            "-p",
            "spectracraft-mono",
            "--test",
            "technical_report_sweep",
        ],
        &stage.root,
        &[("CARGO_TARGET_DIR", target_dir.as_path())],
        UPDATE_CMD_TIMEOUT,
    ) {
        Ok(out) => out,
        Err(err) => {
            return Ok(UpdateActionResult {
                action: "update_now".into(),
                ok: false,
                message: format!(
                    "Could not execute sweep verification ({err}); live workspace was not modified."
                ),
                sweep_test_ran: false,
                sweep_test_passed: false,
                unit_tests_passed: false,
                rolled_back: false,
                rollback_verified: true,
                staged_target: resolved_target.as_ref().map(|s| s.pin.clone()),
                staged_target_detail: resolved_target,
                dismissed_notice_id: None,
            });
        }
    };
    if !sweep.success {
        let reason = if sweep.timed_out {
            "timed out and subprocess was terminated".to_string()
        } else {
            sweep.stderr
        };
        return Ok(UpdateActionResult {
            action: "update_now".into(),
            ok: false,
            message: format!(
                "Mandatory 20 Hz–20 kHz sweep verification failed in staging ({reason}); live Cargo.toml & Cargo.lock were not modified."
            ),
            sweep_test_ran: true,
            sweep_test_passed: false,
            unit_tests_passed: false,
            rolled_back: false,
            rollback_verified: true,
            staged_target: resolved_target.as_ref().map(|s| s.pin.clone()),
            staged_target_detail: resolved_target,
            dismissed_notice_id: None,
        });
    }

    // Mandatory verification gate 2: full library unit tests (including Stage 1 physics + settings_store)
    let unit = match run_command_with_timeout(
        &cargo,
        &[
            "test",
            "--locked",
            "--release",
            "-p",
            "spectracraft-mono",
            "--lib",
        ],
        &stage.root,
        &[("CARGO_TARGET_DIR", target_dir.as_path())],
        UPDATE_CMD_TIMEOUT,
    ) {
        Ok(out) => out,
        Err(err) => {
            return Ok(UpdateActionResult {
                action: "update_now".into(),
                ok: false,
                message: format!(
                    "Could not execute library unit tests ({err}); live workspace was not modified."
                ),
                sweep_test_ran: true,
                sweep_test_passed: true,
                unit_tests_passed: false,
                rolled_back: false,
                rollback_verified: true,
                staged_target: resolved_target.as_ref().map(|s| s.pin.clone()),
                staged_target_detail: resolved_target,
                dismissed_notice_id: None,
            });
        }
    };
    if !unit.success {
        let reason = if unit.timed_out {
            "timed out and subprocess was terminated".to_string()
        } else {
            unit.stderr
        };
        return Ok(UpdateActionResult {
            action: "update_now".into(),
            ok: false,
            message: format!(
                "Rust library unit tests failed in staging ({reason}); live Cargo.toml & Cargo.lock were not modified."
            ),
            sweep_test_ran: true,
            sweep_test_passed: true,
            unit_tests_passed: false,
            rolled_back: false,
            rollback_verified: true,
            staged_target: resolved_target.as_ref().map(|s| s.pin.clone()),
            staged_target_detail: resolved_target,
            dismissed_notice_id: None,
        });
    }

    // All staging verification gates passed -> atomically publish staged manifests to live workspace.
    if pin_changed {
        let staged_toml_bytes = std::fs::read(&stage.cargo_toml_path)?;
        let staged_lock_bytes = std::fs::read(&stage.cargo_lock_path).ok();

        let publish_res: Result<()> = (|| {
            crate::report_export::atomic_write_bytes(&cargo_toml_path, &staged_toml_bytes)?;
            if let Some(ref lock_bytes) = staged_lock_bytes {
                crate::report_export::atomic_write_bytes(&cargo_lock_path, lock_bytes)?;
            }
            Ok(())
        })();

        if let Err(err) = publish_res {
            let verified = restore_and_verify_snapshot(
                &cargo_toml_path,
                &orig_toml_bytes,
                &cargo_lock_path,
                orig_lock_bytes.as_deref(),
            );
            return Ok(UpdateActionResult {
                action: "update_now".into(),
                ok: false,
                message: if verified {
                    format!(
                        "Failed to publish verified manifests ({err}); rolled back and verified original SHA-256 bytes."
                    )
                } else {
                    format!(
                        "Failed to publish verified manifests ({err}); rollback could not verify restored bytes — inspect Cargo.toml and Cargo.lock."
                    )
                },
                sweep_test_ran: true,
                sweep_test_passed: true,
                unit_tests_passed: true,
                rolled_back: verified,
                rollback_verified: verified,
                staged_target: None,
                staged_target_detail: None,
                dismissed_notice_id: None,
            });
        }
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
            "Update verified ({bumped_note}): mandatory 20 Hz–20 kHz sweep test and full Rust unit suite passed 100% in isolated staging before publish."
        ),
        sweep_test_ran: true,
        sweep_test_passed: true,
        unit_tests_passed: true,
        rolled_back: false,
        rollback_verified: false,
        staged_target: None,
        staged_target_detail: None,
        dismissed_notice_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "spectracraft_upd_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

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
    fn m9_staged_target_roundtrips_raw_legacy_and_json_formats() {
        let from_tag = StagedTarget::from_raw("v0.2.1").unwrap();
        assert_eq!(from_tag.pin, "v0.2.1");
        assert_eq!(from_tag.kind, "tag");
        assert_eq!(from_tag.display_label, "craft-libs@v0.2.1");

        // Legacy display string persisted by older versions parses cleanly without failing validation
        let from_legacy = StagedTarget::from_raw("craft-libs@v0.2.1").unwrap();
        assert_eq!(from_legacy, from_tag);

        // Persisted JSON round-trips cleanly
        let json = from_tag.to_persisted_json();
        let from_json = StagedTarget::from_raw(&json).unwrap();
        assert_eq!(from_json, from_tag);

        // Commit SHA sets kind = "rev"
        let from_rev = StagedTarget::from_raw("craft-libs@abcdef123456").unwrap();
        assert_eq!(from_rev.pin, "abcdef123456");
        assert_eq!(from_rev.kind, "rev");

        // Floating branch rejected
        assert!(StagedTarget::from_raw("craft-libs@main").is_err());
    }

    #[test]
    fn h12_exclusive_update_lock_blocks_live_holder_and_reclaims_stale_pid() {
        let dir = temp_dir("lock");
        fn alive_1001(pid: u32) -> bool {
            pid == 1001
        }
        fn none_alive(_pid: u32) -> bool {
            false
        }

        let guard1 = UpdateLockGuard::acquire_with_liveness(&dir, 1001, alive_1001).unwrap();
        // Second caller while PID 1001 is alive must fail
        assert!(UpdateLockGuard::acquire_with_liveness(&dir, 2002, alive_1001).is_err());

        // Simulate crash (forget guard so lock remains on disk) -> dead PID allows reclamation
        std::mem::forget(guard1);
        let guard2 = UpdateLockGuard::acquire_with_liveness(&dir, 2002, none_alive).unwrap();
        drop(guard2);
        assert!(!dir.join(UPDATE_LOCK_FILE).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn h12_missing_cargo_leaves_live_workspace_untouched() {
        let dir = temp_dir("missing_cargo");
        let crate_dir = dir.join("spectracraft-mono");
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        let toml_body = format!(
            "[package]\nname = \"spectracraft-mono\"\n[dependencies]\ncraft-pdf = {{ git = \"https://github.com/storytold/craft-libs\", {CRAFT_LIBS_PIN_KIND} = \"{CRAFT_LIBS_PIN}\" }}\n"
        );
        std::fs::write(dir.join("Cargo.toml"), "[workspace]\nmembers = [\"spectracraft-mono\"]\n").unwrap();
        std::fs::write(crate_dir.join("Cargo.toml"), &toml_body).unwrap();
        std::fs::write(dir.join("Cargo.lock"), "# original lock\n").unwrap();

        let orig_toml_sha = sha256(toml_body.as_bytes());
        let orig_lock_sha = sha256(b"# original lock\n");

        // Point SPECTRACRAFT_CARGO_BIN at nonexistent executable
        std::env::set_var("SPECTRACRAFT_CARGO_BIN", "/nonexistent/spectracraft-cargo-bin");
        let settings_dir = temp_dir("missing_cargo_settings");
        let res = apply_update_now(&dir, Some(&settings_dir), Some("v9.9.9")).unwrap();
        std::env::remove_var("SPECTRACRAFT_CARGO_BIN");

        assert!(!res.ok);
        assert!(res.rollback_verified);
        assert_eq!(sha256(&std::fs::read(crate_dir.join("Cargo.toml")).unwrap()), orig_toml_sha);
        assert_eq!(sha256(&std::fs::read(dir.join("Cargo.lock")).unwrap()), orig_lock_sha);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&settings_dir);
    }

    #[test]
    fn h12_verified_rollback_checks_restored_sha256_bytes() {
        let dir = temp_dir("rollback_verify");
        let toml_path = dir.join("Cargo.toml");
        let lock_path = dir.join("Cargo.lock");
        let orig_toml = b"orig-toml-bytes";
        let orig_lock = b"orig-lock-bytes";
        std::fs::write(&toml_path, b"corrupted-partial-write").unwrap();
        std::fs::write(&lock_path, b"corrupted-lock").unwrap();

        let ok = restore_and_verify_snapshot(&toml_path, orig_toml, &lock_path, Some(orig_lock));
        assert!(ok);
        assert_eq!(std::fs::read(&toml_path).unwrap(), orig_toml);
        assert_eq!(std::fs::read(&lock_path).unwrap(), orig_lock);

        let _ = std::fs::remove_dir_all(&dir);
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
