# CLAUDE.md

## Project

FileCommand is a keyboard-driven, dual-panel file manager for the terminal, written in Rust (ratatui + crossterm). It recreates the Norton Commander 5.5 look and workflow with a small set of modern extras. Windows-first; cross-platform builds are best-effort.

The full application specification lives at `docs/superpowers/specs/2026-08-06-filecommand-design.md`. The project is pre-implementation — no Rust code exists yet.

## Git workflow (strict)

- **Nothing goes to `main` without explicit user confirmation.** No exceptions. Changes reach `main` via pull request, and a PR is created or merged only after the user gives the go-ahead.
- **Never edit on `main`.** All work — every edit, of any kind — happens on a branch.
- **`Spec` branch** — application specification and research (spec documents, research notes/images). OpenSpec proposals may be drafted here.
- **OpenSpec proposals** — authored with the OpenSpec CLI (`openspec/` directory, once initialized). Proposals can be created on the `Spec` branch, but they must be merged to `main` before any implementation branch for them is created.
- **Implementation branches** — named `build/<batch-name>`, always branched off `main`, and implement one or more approved OpenSpec proposals.
- **Ultracode** — the user may invoke "ultracode" to build a batch of proposals. That is expected; the batch is built on its `build/` branch off `main` like any other implementation work.

In short: research and proposals flow through `Spec` → `main` (PR, user-confirmed); implementation flows through `build/<batch-name>` off `main` → `main` (PR, user-confirmed).
