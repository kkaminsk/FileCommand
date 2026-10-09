# Tasks: hidden-entry-styling

## 1. Listing (`crates/filecommand-core/src/listing.rs`)

- [ ] 1.1 `RawDirEntry.hidden`, `Entry.hidden` (`Entry::parent_dir()` → `false`); `From<RawDirEntry>` copies it (panel-navigation "Entry row rendering": "Hidden entries render dimmed")
- [ ] 1.2 `StdFsReader::read_dir`: `cfg(windows)` attribute test (`0x2 | 0x4`), else dot-prefix (design D1)
- [ ] 1.3 Tests: fake reader entries with the flag survive sorting and quick-filter narrowing; `..` is never hidden

## 2. Theme (`crates/filecommand-core/src/theme.rs`)

- [ ] 2.1 `Role::PanelHidden` → `"panel.hidden"`; add to `ALL_ROLES`; six built-in tables per design D2 (theme-system "`panel.hidden` role")
- [ ] 2.2 Tests: every built-in defines it; it differs from `panel.file` in every theme; `Theme::build` rejects a table missing it (scenario "Missing role is rejected")

## 3. Renderer (`crates/filecommand-tui/src/views/panel.rs`)

- [ ] 3.1 Full and Brief entry style resolution per design D3 (scenarios "Selected hidden entry shows selected style", "Cursor bar wins"; additional-panel-modes "Brief mode dims hidden entries")
- [ ] 3.2 Snapshot test: a listing with a hidden file, a hidden directory, a selected hidden file, and the cursor on a hidden entry (Full and Brief)

## 4. Docs

- [ ] 4.1 README "Themes": `panel.hidden` in the role list; "Panel display modes": hidden/system entries render dimmed

## 5. Verify

- [ ] 5.1 `cargo test --workspace` green; `openspec validate hidden-entry-styling --strict` clean
- [ ] 5.2 Manual: browse `C:\` (`$RECYCLE.BIN`, `System Volume Information` dimmed) and a repo (`.git` dimmed) in each built-in theme
