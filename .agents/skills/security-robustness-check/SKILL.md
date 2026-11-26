---
name: security-robustness-check
description: Standing security, file-handling, and pre-export robustness checklist for SpectraCraft. Run whenever the user requests a security, robustness, or pre-export audit, or whenever new file-handling, upload, IPC, or DAW export code is added.
---

# SpectraCraft Security & Robustness Check

Authoritative mirror of `/SPECTRACRAFT_BIBLE.md` Section 09. Follow this checklist on every security/robustness/pre-export audit and whenever file-handling, upload, IPC, or export code changes.

## Standing Constraints

- **Prefer Rust** for engine, IPC (`src-tauri/src/lib.rs`), and interchange (`spectracraft-mono/src/daw_bridge.rs`) fixes. Keep TypeScript strictly a thin viewer/control surface.
- **Do not alter protected audio invariants**: never change Stage 1 persistent-state physics (`string_state.rs`), additive re-pick, continuous bend, zero original PCM leakage, or blind Before/After reporting (`technical_report.rs`).
- **Make only safe, obvious fixes** unless broader changes are explicitly requested.
- **Mark local-only Rust verification as pending** (`cargo test`, `cargo clippy`, `cargo audit`, `cargo build --release`, `SOURCE-SHA256.json`) when running in a cloud sandbox without a full Rust toolchain.

## Checklist

### Critical
1. **No secrets or API keys** in frontend TypeScript, CSS, or committed files.
2. **No database or sensitive queries in the browser**. All real work stays in the Rust engine or `server.ts`.
3. **WAV upload validation**: validate type (RIFF/WAVE + `fmt ` PCM/IEEE-float, mono, 8–96 kHz, valid `data` chunk) and size before bytes reach the Rust binary.
4. **Zero original PCM / bedroom noise leakage**: verify Stage 1 (`reperform.rs`, `string_state.rs`) and DAW export stems (`daw_bridge.rs`, `server.ts` `/daw-export`) never open or blend original PCM (`original_audio_opened: false`).
5. **Private, cleaned temp files and session folders**: create temp/session dirs owner-only (`0700`), uploaded/temp files owner-only (`0400`/`0600`), clean up rejected/evicted/stale folders, never leave temp dirs world-readable.

### High Priority
6. **Server-side input validation** on all inputs reaching the Rust CLI or Express endpoints (use `execFile` argument arrays, never shell interpolation).
7. **Path traversal protection** on any file paths accepted from UI, CLI, or IPC (reject `..`, NUL/control chars, enforce directory confinement).
8. **Error message hygiene**: errors from `server.ts` and Rust binaries must not leak internal filesystem paths or stack traces to clients.
9. **API & CORS posture**: no wide-open wildcard CORS; enforce same-origin checks on mutating `/api/*` endpoints.
10. **Dependency health & pinning**: check `Cargo.toml`, `src-tauri/Cargo.toml`, `package.json`, and lockfiles (`bun audit` / `cargo audit`) for known vulnerabilities and unpinned ranges.
11. **DAW interchange sanitization**: `daw_bridge.rs` must never write original audio or unsanitized user data (control chars, XML metacharacters, CSV/TSV formula prefixes) into exported `.sesx`, `.csv`, `.tsv`, or `.scraft.json` files.

### Medium Priority
12. **Uncommitted local config**: `.env`, `.env.local`, and `config.toml` are gitignored and not committed.
13. **Minimal logging**: server and Rust logs do not record full audio paths or user data unnecessarily.
14. **Bounded caches & session maps**: `server.ts` `sessions`, per-session artifact maps, and `mediaRegistry` enforce upper bounds with eviction and disk cleanup.
15. **Tauri & desktop IPC path checks**: `src-tauri/src/lib.rs` and `desktop.rs` validate file paths (extension, no `..`, no control chars, regular file / safe output directory) before reading or writing.

## Output Format
Report findings grouped by **Critical**, **High**, and **Medium**, list exact Rust and TypeScript files modified, note verified live checks, and end with pending local Rust verification commands.
