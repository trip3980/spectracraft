//! Platform checks are evidence, not promises of device/runtime support.
use crate::Result;
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub fn targets(platform: &str) -> Result<&'static [&'static str]> {
    Ok(match platform {
        "ios" => &["aarch64-apple-ios", "aarch64-apple-ios-sim"],
        "android" => &["aarch64-linux-android", "x86_64-linux-android"],
        "macos" => &["aarch64-apple-darwin", "x86_64-apple-darwin"],
        "windows" => &["x86_64-pc-windows-msvc"],
        "linux" => &["x86_64-unknown-linux-gnu"],
        _ => return Err("Unknown platform".into()),
    })
}
fn run(root: &Path, args: &[&str]) -> serde_json::Value {
    let mut child = match Command::new("cargo")
        .args(args)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return serde_json::json!({"command":args,"status":"blocked","reason":e.to_string()})
        }
    };
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let read = |pipe: &mut dyn Read| {
        let mut output = Vec::new();
        let mut block = [0; 8192];
        while let Ok(n) = pipe.read(&mut block) {
            if n == 0 {
                break;
            }
            output.extend_from_slice(&block[..n]);
            if output.len() > 65536 {
                output.drain(..output.len() - 65536);
            }
        }
        String::from_utf8_lossy(&output).into_owned()
    };
    let stdout = std::thread::spawn(move || read(&mut out));
    let stderr = std::thread::spawn(move || read(&mut err));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() < Duration::from_secs(900) => {
                std::thread::sleep(Duration::from_millis(50))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    serde_json::json!({"command":args,"status":status.map(|s|if s.success(){"passed"}else{"failed"}).unwrap_or("blocked"),"exit":status.and_then(|s|s.code()),"stdout":stdout.join().unwrap_or_default(),"stderr":stderr.join().unwrap_or_default()})
}
pub fn check(
    root: &Path,
    platform: &str,
    desktop: bool,
    advisories: bool,
) -> Result<serde_json::Value> {
    let root = root.canonicalize()?;
    let host = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "linux" => "linux",
        _ => "unsupported",
    };
    if desktop && platform != host {
        return Err("Host desktop checks require that operating system".into());
    }
    let meta = Command::new("cargo")
        .args([
            "metadata",
            "--locked",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
        ])
        .current_dir(&root)
        .output()?;
    if !meta.status.success() {
        return Err(String::from_utf8_lossy(&meta.stderr).into_owned().into());
    }
    let meta: serde_json::Value = serde_json::from_slice(&meta.stdout)?;
    let package = meta["packages"]
        .as_array()
        .ok_or("Missing packages")?
        .iter()
        .find(|p| p["name"] == "spectracraft-mono")
        .ok_or("Wrong crate")?;
    let deps = package["dependencies"]
        .as_array()
        .ok_or("Missing dependencies")?;
    let unconditional: Vec<_> = deps
        .iter()
        .filter(|d| {
            ["eframe", "rodio", "rfd"]
                .iter()
                .any(|name| d["name"] == *name)
                && d["optional"] != true
        })
        .map(|d| d["name"].clone())
        .collect();
    let mut checks = Vec::new();
    for target in targets(platform)? {
        checks.push(run(
            &root,
            &[
                "check",
                "--locked",
                "--offline",
                "--lib",
                "--no-default-features",
                "--target",
                target,
            ],
        ));
    }
    if desktop {
        checks.push(run(
            &root,
            &["test", "--locked", "--offline", "--features", "desktop"],
        ));
        checks.push(run(
            &root,
            &[
                "build",
                "--locked",
                "--offline",
                "--features",
                "desktop",
                "--bin",
                "spectracraft-desktop",
            ],
        ));
    }
    if advisories {
        checks.push(run(&root, &["audit", "--no-fetch"]));
    }
    let passed = unconditional.is_empty() && checks.iter().all(|v| v["status"] == "passed");
    Ok(
        serde_json::json!({"platform":platform,"manifest":{"unconditionally_linked_shell_dependencies":unconditional,"default_features":package["features"]["default"],"mobile_builds_verified":false},"checks":checks,"passed":passed,"native_device_smoke":"not run by this script","audio_quality":"not assessed","advisories_requested":advisories}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matrix_distinguishes_host_and_core_targets() {
        assert_eq!(targets("windows").unwrap(), ["x86_64-pc-windows-msvc"]);
        assert_eq!(targets("ios").unwrap().len(), 2);
        assert!(targets("guess").is_err());
    }
}
