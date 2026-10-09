# Tasks: open-with-associated-app

## 1. Core (`crates/filecommand-core`)

- [ ] 1.1 `dialogs.rs`: `FileActionMenuEntry::Open` with label `Open`/hotkey `O`; ordering per design D2 for file, executable, and directory targets (file-action-menu "Menu contents, ordering, and navigation"; "Directory targets and selection-scoped invocation")
- [ ] 1.2 `update.rs`: `Effect::OpenWithAssociation { path: PathBuf, cwd: PathBuf }`; Open routes to it single-target (design D3); `Command::OpenWithAssociationFailed { message }` sets the active panel's `last_error` (file-action-menu "Open with the associated application")
- [ ] 1.3 Tests: menu order and highlight for the three target kinds; `O` hotkey; Open on a selected target emits one effect for that target only; the failure command lands on `last_error`

## 2. TUI (`crates/filecommand-tui`)

- [ ] 2.1 `Cargo.toml`: `[target.'cfg(windows)'.dependencies] windows-sys = { version = "0.59", features = ["Win32_UI_Shell", "Win32_Foundation", "Win32_UI_WindowsAndMessaging"] }`
- [ ] 2.2 `app.rs`: `open_with_association(path, cwd) -> Result<(), String>`; Windows `ShellExecuteW` with the `\\?\` prefix stripped (reuse the clipboard-export helper) and error-code mapping per design D1; non-Windows detached `xdg-open`/`open`
- [ ] 2.3 Dispatch the effect without suspending the terminal; map `Err` to `Command::OpenWithAssociationFailed`
- [ ] 2.4 Snapshot tests: file menu with Open first and highlighted; executable menu with Run then Open; directory menu with Open first

## 3. Docs

- [ ] 3.1 README: file-action menu entry lists (Enter and right-click), "Running programs" section explains Open vs Run (design D4); F1 Help "File operations" page

## 4. Verify

- [ ] 4.1 `cargo test --workspace` green; `openspec validate open-with-associated-app --strict` clean
- [ ] 4.2 Manual: Enter, Enter on a `.pdf` opens the reader while FileCommand stays live; right-click a directory → Open shows it in Explorer; a file with an unknown extension reports the no-association message on the status line
