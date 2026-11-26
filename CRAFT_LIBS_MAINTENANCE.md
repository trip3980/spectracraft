# `craft-libs` & ArtCraft Maintenance Guide

SpectraCraft uses crates from [`storytold/craft-libs`](https://github.com/storytold/craft-libs) through explicit, non-floating Git pins and a single thin compatibility module so upstream updates are deliberate, diffable, and low-risk.

---

## 1. Current Pinned Dependencies

Declared in `spectracraft-mono/Cargo.toml` (allowed in `spectracraft-mono/deny.toml` under `[sources].allow-git`):

```toml
craft-atomic   = { git = "https://github.com/storytold/craft-libs", rev = "05b99ee5dcd64f2c954a693ef361301214ddc3c7" }
craft-pdf      = { git = "https://github.com/storytold/craft-libs", rev = "05b99ee5dcd64f2c954a693ef361301214ddc3c7", default-features = false }
craft-settings = { git = "https://github.com/storytold/craft-libs", rev = "05b99ee5dcd64f2c954a693ef361301214ddc3c7" }
```

- **Never track a floating branch** (`branch = "main"` is forbidden).
- `storytold/craft-libs` does not currently publish semver Git tags; therefore pins use an explicit commit `rev`. Once release tags exist (e.g. `v0.1.0`), replace `rev = "..."` with `tag = "v0.1.0"`.
- Builds never auto-update `craft-libs` and never fail merely because a newer tag or commit exists upstream.

---

## 2. Thin Compatibility Isolation Surfaces (`report_export.rs` & `settings_store.rs`)

All direct calls into `craft-libs` crates are isolated inside two thin internal wrapper modules:
- **`spectracraft-mono/src/report_export.rs`** — isolates `craft_pdf` and `craft_atomic`.
- **`spectracraft-mono/src/settings_store.rs`** — isolates `craft_settings` (local, versioned, uncommitted `config.toml` that preserves unknown ArtCraft suite keys).

No other SpectraCraft module imports `craft_pdf`, `craft_atomic`, or `craft_settings`.

### Exact Upstream Symbols Currently Consumed

| Crate | Wrapper Module | Symbols Consumed |
| --- | --- | --- |
| `craft-pdf` (`default-features = false`) | `report_export.rs` | `craft_pdf::classic::Writer::{with_capacity, object, stream, finish}`, `craft_pdf::classic::Error` |
| `craft-atomic` | `report_export.rs` | `craft_atomic::atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()>` |
| `craft-settings` | `settings_store.rs` | `craft_settings::{Settings, Location, SettingsStore, DocumentMut, MigrateError}` |

If a future `craft-libs` release changes any of these signatures, only `report_export.rs` or `settings_store.rs` needs to be adapted; the blind report analyzer (`technical_report.rs`), Stage 1 dry physics (`string_state.rs`, `reperform.rs`), pipeline (`pipeline.rs`), UI viewer (`TechnicalReportModal.tsx`), and CLI (`spectracraft-qa.rs`) remain untouched.

---

## 3. Manual Update-Check & Pin-Bump Procedure

### Step 1 — Check Current Pins & Compare Against Upstream (Launch or CLI)

On startup (opt-in via `settings.updates.check_on_launch` in local `config.toml`), SpectraCraft runs a non-blocking background update check for pinned `craft-libs` crates and direct `Cargo.toml` dependencies. When updates are found, a non-modal banner offers three explicit choices:
1. **`Update now`** — bumps explicit pins for selected crates, runs `cargo update`, and requires the mandatory 10-second 20 Hz–20 kHz sweep test and Rust unit suite to pass before completing (rolls back `Cargo.toml` / `Cargo.lock` automatically on failure).
2. **`Download and update later`** — runs `cargo fetch --locked` only and records the staged target until explicit confirmation.
3. **`Not now`** — does nothing and remembers dismissal for the session/version until next launch or a newer release.

From the CLI, run the read-only informational checker (never modifies files or fails builds):

```sh
# Offline summary of current pins, consumed symbols, and shared ArtCraft crates:
./target/release/spectracraft-qa check-craft-libs
./target/release/spectracraft-qa update-check .

# Online comparison against storytold/craft-libs tags/main HEAD and direct Cargo.toml crates:
./target/release/spectracraft-qa check-craft-libs --online
./target/release/spectracraft-qa update-check . --online
```

Or query upstream directly with Git:

```sh
git ls-remote --tags --heads https://github.com/storytold/craft-libs.git
```

### Step 2 — Deliberate Pin Bump

1. Diff upstream changes for the exact consumed symbols listed in Section 2 (`crates/pdf/src/classic.rs`, `crates/atomic/src/lib.rs`, `crates/settings/src/lib.rs`).
2. Update the explicit `tag = "vX.Y.Z"` (or `rev = "<commit>"`) for `craft-pdf`, `craft-atomic`, and `craft-settings` in `spectracraft-mono/Cargo.toml` and update `CRAFT_LIBS_PIN_KIND` / `CRAFT_LIBS_PIN` in `spectracraft-mono/src/report_export.rs`.
3. Refresh the lockfile deliberately:
   ```sh
   cargo update -p craft-pdf -p craft-atomic -p craft-settings
   ```
4. If an upstream API changed, update only `spectracraft-mono/src/report_export.rs` or `spectracraft-mono/src/settings_store.rs`. Do not touch Stage 1 dry physics (`string_state.rs`, `reperform.rs`, `cue_passes.rs`).

### Step 3 — Mandatory Verification Gate (Must Pass 100%)

Every `craft-libs` pin bump must pass **all** checks below before merging:

```sh
# 1. Release build + refresh source fingerprint inventory
cargo build --release --locked
./target/release/spectracraft-qa source-inventory spectracraft-mono > spectracraft-mono/SOURCE-SHA256.json
./target/release/spectracraft-qa verify-source spectracraft-mono spectracraft-mono/SOURCE-SHA256.json

# 2. Mandatory 10-second 20 Hz–20 kHz sine sweep report, viewer plot, and export test
cargo test --locked --release -p spectracraft-mono --test technical_report_sweep -- --nocapture

# 3. Full Rust library + integration suites & lint
cargo test --locked -p spectracraft-mono --lib
cargo clippy --locked --lib --bins --tests

# 4. TypeScript unit tests + web build
npx --yes tsx --test src/lib/*.test.ts
npm run lint
npm run build
```

Finally, verify in the running dark-mode viewer that waveform, spectrograms, difference spectrogram, 1/3-octave bars, upper-band plot, PDF export (`craft-pdf`), and DOCX fallback remain readable.

---

## 4. Shared ArtCraft Integration Points

If SpectraCraft later shares document models, timelines, preferences, or desktop widgets with other ArtCraft applications (**PhotoCraft**, **FilmCraft**, **DeckCraft**, etc.), adopt the shared crates in `storytold/craft-libs` instead of duplicating types:

| Shared Crate | Domain | Current Status | Integration Guidance |
| --- | --- | --- | --- |
| `craft-pdf` | Dark-mode PDF white papers & embedded plots | **Active** (`report_export.rs`) | Keep all calls behind `report_export.rs`. |
| `craft-atomic` | Crash-safe atomic file writes | **Active** (`report_export.rs`) | Route all WAV/JSON/PDF/MD writes through `report_export::atomic_write_bytes`. |
| `craft-wordmodel` | Shared structured document model | Stable future integration point | Adopt before enabling native DOCX export instead of building a custom document AST. |
| `craft-docx` | Word (`.docx`) export | Deferred (Markdown fallback active) | Wire inside `report_export.rs` once `craft-wordmodel`/`craft-opc` dependency budget is approved. |
| `craft-timeline` | Shared timeline tracks, clips, markers | Stable future integration point | Adopt if SpectraCraft expands into multi-clip timelines shared with FilmCraft. |
| `craft-ui` | Shared ArtCraft desktop widgets & theme | Future integration point | Adopt once `spectracraft-desktop` aligns its `egui`/`eframe` version with `craft-libs`. |
| `craft-settings` | Cross-app settings & preference storage | **Active** (`settings_store.rs`) | Stores local uncommitted `config.toml` with versioned schema & unknown-key preservation. |
| `craft-audio` / `craft-audio-dsp` | Shared audio container / utility DSP | Evaluate selectively | Never replace or alter SpectraCraft Stage 1 dry physics invariants. |
