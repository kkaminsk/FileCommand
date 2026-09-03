# CLAUDE.md

## Project

FileCommand is a keyboard-driven, dual-panel file manager for the terminal, written in Rust (ratatui + crossterm). It recreates the Norton Commander 5.5 look and workflow with a small set of modern extras. Windows-first; cross-platform builds are best-effort.

The full application specification lives at `docs/superpowers/specs/2026-08-06-filecommand-design.md`. Milestones M1–M5 are implemented in `crates/filecommand-core` (state/reducer, no UI deps) and `crates/filecommand-tui` (rendering); the OpenSpec specs under `openspec/specs/` are the source of truth for current behavior. `installer/` holds the WiX (v4/v5) MSI + Burn bootstrapper packaging (`installer/README.md` has build steps and scope semantics); see the [`windows-installer` capability spec](openspec/changes/wix-installer/specs/windows-installer/spec.md) for its behavior.

## Git workflow (strict)

- **Nothing goes to `main` without explicit user confirmation.** No exceptions. Changes reach `main` via pull request, and a PR is created or merged only after the user gives the go-ahead.
- **Never edit on `main`.** All work — every edit, of any kind — happens on a branch.
- **`Spec` branch** — application specification and research (spec documents, research notes/images). OpenSpec proposals may be drafted here.
- **OpenSpec proposals** — authored with the OpenSpec CLI (`openspec/` directory, once initialized). Proposals can be created on the `Spec` branch, but they must be merged to `main` before any implementation branch for them is created.
- **Implementation branches** — named `build/<batch-name>`, always branched off `main`, and implement one or more approved OpenSpec proposals.
- **Ultracode** — the user may invoke "ultracode" to build a batch of proposals. That is expected; the batch is built on its `build/` branch off `main` like any other implementation work.

In short: research and proposals flow through `Spec` → `main` (PR, user-confirmed); implementation flows through `build/<batch-name>` off `main` → `main` (PR, user-confirmed).

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **FileCommand** (5101 symbols, 13040 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> Index stale? Run `node .gitnexus/run.cjs analyze` from the project root — it auto-selects an available runner. No `.gitnexus/run.cjs` yet? `npx gitnexus analyze` (npm 11 crash → `npm i -g gitnexus`; #1939).

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows. For regression review, compare against the default branch: `detect_changes({scope: "compare", base_ref: "main"})`.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `query({search_query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `context({name: "symbolName"})`.
- For security review, `explain({target: "fileOrSymbol"})` lists taint findings (source→sink flows; needs `analyze --pdg`).

## Never Do

- NEVER edit a function, class, or method without first running `impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `rename` which understands the call graph.
- NEVER commit changes without running `detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/FileCommand/context` | Codebase overview, check index freshness |
| `gitnexus://repo/FileCommand/clusters` | All functional areas |
| `gitnexus://repo/FileCommand/processes` | All execution flows |
| `gitnexus://repo/FileCommand/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
