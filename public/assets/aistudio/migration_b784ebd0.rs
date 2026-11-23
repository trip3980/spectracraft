//! Inventory only: never deletes or modifies a retired script.
use crate::{digest::sha256, Result};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
fn files(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(root)? {
        let e = entry?;
        let kind = e.file_type()?;
        if kind.is_symlink() {
            return Err(format!("Refusing ambiguous symlink: {}", e.path().display()).into());
        }
        if kind.is_dir() {
            files(&e.path(), out)?;
        } else if kind.is_file() {
            out.push(e.path());
        }
    }
    Ok(())
}
pub fn audit(retired: &Path, active: &Path) -> Result<serde_json::Value> {
    let mut expected = BTreeMap::new();
    for line in include_str!("../MIGRATION.tsv").lines().skip(1) {
        let cells: Vec<_> = line.split('\t').collect();
        if cells.len() != 4 {
            return Err("Malformed migration ledger".into());
        }
        if expected
            .insert(cells[0], (cells[1], cells[2], cells[3]))
            .is_some()
        {
            return Err("Duplicate retired path".into());
        }
    }
    let mut found = Vec::new();
    files(retired, &mut found)?;
    found.sort();
    let mut rows = Vec::new();
    for file in found
        .into_iter()
        .filter(|p| p.extension().is_some_and(|s| s == "py"))
    {
        let relative = file
            .strip_prefix(retired)?
            .to_str()
            .ok_or("Non-UTF8 path")?
            .replace('\\', "/");
        let Some((status, replacement, remaining)) = expected.remove(relative.as_str()) else {
            return Err(format!("Unaccounted retired script: {relative}").into());
        };
        let data = std::fs::read(&file)?;
        rows.push(serde_json::json!({"path":relative,"sha256":sha256(&data),"bytes":data.len(),"status":status,"replacement":replacement,"remaining":remaining,"deletion_performed":false}));
    }
    for line in include_str!("../RETIRED-DELETIONS.tsv").lines().skip(1) {
        let (path, hash) = line.split_once("\t").ok_or("Malformed deletion receipt")?;
        if let Some((status, replacement, remaining)) = expected.remove(path) {
            if status != "replaced" {
                return Err("Deletion receipt must reference a completed replacement".into());
            }
            rows.push(serde_json::json!({"path":path,"sha256":hash,"status":status,"replacement":replacement,"remaining":remaining,"presence":"intentionally removed by owner; baseline hash retained","deletion_performed":false}));
        }
    }
    if !expected.is_empty() {
        return Err(format!("Missing retired scripts: {:?}", expected.keys()).into());
    }
    let mut project = Vec::new();
    for name in ["src", "tests"] {
        files(&active.join(name), &mut project)?;
    }
    let non_rust: Vec<_> = project
        .iter()
        .filter(|p| p.extension().is_none_or(|e| e != "rs"))
        .map(|p| p.display().to_string())
        .collect();
    if !non_rust.is_empty() {
        return Err(format!("Non-Rust source files: {non_rust:?}").into());
    }
    Ok(
        serde_json::json!({"retired_script_count":rows.len(),"active_source_files":project.len(),"active_source_rust_only":true,"scripts":rows,"policy":"This audit never deletes; documented owner removals retain baseline hashes; partial and pending sources must remain"}),
    )
}

pub fn source_inventory(root: &Path) -> Result<serde_json::Value> {
    let mut paths = Vec::new();
    for folder in ["src", "tests"] {
        files(&root.join(folder), &mut paths)?;
    }
    for name in [
        "Cargo.toml",
        "Cargo.lock",
        "README.md",
        "NATIVE-GUITAR.md",
        "REPRODUCE.md",
        "THIRD-PARTY.md",
        "MIGRATION.tsv",
        "RETIRED-DELETIONS.tsv",
    ] {
        paths.push(root.join(name));
    }
    paths.sort();
    let mut hashes = BTreeMap::new();
    for path in paths {
        hashes.insert(
            path.strip_prefix(root)?
                .to_str()
                .ok_or("Non-UTF8 source path")?
                .replace('\\', "/"),
            sha256(&std::fs::read(path)?),
        );
    }
    Ok(serde_json::to_value(hashes)?)
}
