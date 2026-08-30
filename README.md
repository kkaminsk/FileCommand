# FileCommand

A keyboard-driven, dual-panel file manager for the terminal, written in Rust
([ratatui](https://github.com/ratatui/ratatui) + [crossterm](https://github.com/crossterm-rs/crossterm)).
It recreates the Norton Commander 5.5 look and workflow — function-key
command bar, pulldown menus, dual panels, built-in viewer/editor — with a
small set of modern extras layered on top. Windows-first; cross-platform
builds are best-effort.

## Features

- **Dual-panel navigation** — Full/Brief/Tree panel modes, adjustable split,
  panel tabs, quick filter, type-ahead and fuzzy jump, viewport scrolling
  with a right-border scrollbar.
- **File operations** — copy/move/rename/delete with confirmation dialogs,
  multi-selection, clipboard file export to other Windows applications,
  Windows Explorer-style error handling.
- **Command line & menus** — an editable command line, pulldown menus, a
  drive selector, and an Enter-triggered file-action menu
  (run/view/edit/copy/move/rename/delete).
- **Built-in viewer & editor**, plus launching an external editor.
- **Visual themes** — terminal green, purple lights, yellow storm, and
  inverted, switchable live from the Options → Themes picker or the F2 user
  menu, or pinned at launch with `--theme`.
- **Mouse support** — click/drag panel focus and selection, resizable split.
- **Responsive layout** — degrades gracefully down to a 60×16 terminal.
- **Git info, find-file, and a startup splash screen.**

See `openspec/specs/` for the authoritative, per-capability behavior specs,
and `docs/superpowers/specs/2026-08-06-filecommand-design.md` for the full
original design document.

## Building from source

Requires the [Rust toolchain](https://rustup.rs) (stable).

```powershell
cargo build --release
```

The binary is produced at `target\release\filecommand.exe`. Run it directly,
or via:

```powershell
cargo run --release
```

Run the test suite (state/reducer logic in `filecommand-core`, rendering and
snapshot tests in `filecommand-tui`):

```powershell
cargo test --workspace
```

## Installing

A Windows installer (`FileCommandSetup.exe`) packages `filecommand.exe` into
a per-user (no elevation) or per-machine (elevated) install, adds itself to
`PATH`, and is winget-deployable. See [`installer/README.md`](installer/README.md)
for build steps, scope semantics, and winget manifest details.

## Command-line options

| Flag | Effect |
|------|--------|
| `--theme <name>` / `--theme=<name>` | Launch with a specific theme (`terminal-green`, `purple-lights`, `yellow-storm`, `inverted`), overriding the saved default for this session. |
| `--nosplash` | Skip the startup splash screen. |
| `--nomouse` | Disable mouse capture for this session (mouse support otherwise follows `config.toml`). |

## Project layout

- `crates/filecommand-core` — state and reducer logic, no UI dependencies.
- `crates/filecommand-tui` — rendering, input handling, and the `filecommand`
  binary.
- `openspec/` — OpenSpec proposals and the per-capability specs that are the
  source of truth for current behavior.
- `installer/` — the WiX (v4/v5) MSI + Burn bootstrapper packaging.

## Contributing

This project develops behind an OpenSpec-driven, branch-per-change
workflow — see `CLAUDE.md` for the exact rules (branch naming, what can land
on `main`, and how proposals move from spec to implementation).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option.
