# SpectraCraft — Gemini CLI & Antigravity Project Guide

This repository hosts **SpectraCraft** (`spectracraft-mono/`), a native Rust desktop audio workstation and CLI/QA engine.

## Authoritative Project Rules

1. **Architecture & Invariants**: `/SPECTRACRAFT_SPEC.md` is authoritative. Never replace the native Rust workstation with a browser substitute and never alter the five-stage pipeline order:
   `Stage 1: Dry Regeneration -> Stage 2: Surgical Region Editing (dry only) -> Stage 3: Optional Microphone -> Stage 4: Optional Space/Room -> Stage 5: Optional Plugin Slot`.
2. **Working Rules & Enforcement Contract**: Follow `/SPECTRACRAFT_BIBLE.md` Sections 00–20 (`SC-*` requirement IDs) and maintain `/AUDIT_PROGRESS.md` as the implementation and evidence ledger. Keep edits minimal and clean (Section 00 Ponytail rules), preserve all nine invariants, never cut validation or audio/state tests, reconcile legacy gate rules with canonical Section 14 Candidate policy (`SC-GOV-04`), and end changes with one line stating what was skipped or left unchecked.
3. **Rust-First Engine Rule** (`/SPECTRACRAFT_BIBLE.md` Section 01): Prefer Rust (`spectracraft-mono/` and Rust engine crates) unless the task is explicitly UI-only; keep TypeScript strictly a thin viewer/control surface; never move engine logic, Stage 1 persistent-state physics, additive re-pick, continuous bend, DSP, or blind reporting into TypeScript; break ties in favor of Rust to preserve engine ownership; always name the Rust files/modules and adapters changed.
4. **Standing Security & Robustness Checklist** (`/SPECTRACRAFT_BIBLE.md` Sections 09, 15, 16 & `.agents/skills/security-robustness-check/SKILL.md`): Run the Critical / High / Medium security and robustness checklist on any security, robustness, or pre-export audit and whenever new file-handling, upload, IPC, or export code is added. Prefer Rust for engine/IPC fixes, never alter Stage 1 physics or blind reporting, and record unavailable toolchain or platform checks as `NOT RUN` with the concrete missing prerequisite.
5. **Evidence & Resumable Handoff** (`/SPECTRACRAFT_BIBLE.md` Sections 10, 18, 19 & `/AUDIT_PROGRESS.md`): Keep implementation status (`Pending` / `In Progress` / `Implemented` / `Verified` / `Blocked`) separate from remediation verification (`PASS` / `FAIL` / `NOT RUN`). Never mark a finding `Verified` without passing observable acceptance evidence for the exact tested revision and surface.


## Installed Agent Skills (`.agents/skills/`)

Project-local skills are installed in copy mode via `skills.sh` and pinned in `skills-lock.json` for **Gemini CLI** and **Antigravity**:

- **SpectraCraft standing skill** (repository-local):
  `security-robustness-check` (`.agents/skills/security-robustness-check/SKILL.md`)
- **obra/superpowers** (MIT License — `https://github.com/obra/superpowers`):
  `systematic-debugging`, `test-driven-development`, `verification-before-completion`, `requesting-code-review`, `subagent-driven-development`
- **trailofbits/skills** (CC BY-SA 4.0 License — `https://github.com/trailofbits/skills`):
  `rust-review`, `dimensional-analysis`, `property-based-testing`, `differential-review`, `spec-to-code-compliance`
- **full-stack-skills/rust-skills** (Apache-2.0 License — `https://github.com/full-stack-skills/rust-skills`):
  `rust-stable`, `rust-api-design`, `rust-testing`, `rust-style-clippy`, `rust-code-review`

### Host Compatibility Notes

- **Google AI Studio Build**: Does not automatically discover workspace `.agents/skills/` files; apply the workflows manually or run them from Gemini CLI / Antigravity.
- **Gemini CLI / Antigravity**: Discover `.agents/skills/<name>/SKILL.md` automatically. Restore from lockfile with `npx skills experimental_install`. For full upstream Superpowers extension hooks, users may also run `gemini extensions install https://github.com/obra/superpowers` or `agy plugin install https://github.com/obra/superpowers`.
- **Trail of Bits Claude/Codex Plugin Caveats**:
  - `rust-review` references plugin-level `scripts/*.py`, `prompts/`, and `agents/*.md` outside `skills/rust-review/` that are not copied by `skills.sh`; use its `SKILL.md` checklist in single-agent mode or install the full plugin in Claude Code.
  - `subagent-driven-development`, `requesting-code-review`, `dimensional-analysis`, and `spec-to-code-compliance` assume subagent dispatch; in single-agent environments without subagent tools, execute their steps sequentially in-context and note the adaptation.

## Standard Rust Verification Commands

```bash
cargo test --locked --offline
cargo clippy --locked --offline --lib --bins --tests
cargo fmt --all --check
```
