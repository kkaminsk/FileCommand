# Change: hidden-entry-styling

## Why

Listings show every entry, including Windows hidden and system files and directories (`.git`, `desktop.ini`, `$RECYCLE.BIN`, `System Volume Information`), styled exactly like ordinary entries. `RawDirEntry` keeps no attribute bits even though the enumeration metadata already carries them (`fs.rs` reads `file_attributes()` for readonly/reparse). Users cannot tell at a glance which entries are the ones Explorer hides. The user chose to keep showing them but in a distinct color — no toggle.

## What Changes

- `Entry` gains `hidden: bool`, set from the enumeration: on Windows when `FILE_ATTRIBUTE_HIDDEN` or `FILE_ATTRIBUTE_SYSTEM` is set; elsewhere when the name starts with `.`. `..` is never hidden.
- A new theme role **`panel.hidden`** is added to every built-in theme with a visibly dimmer color than `panel.file` on the same background; hidden files and hidden directories both render with it in Full and Brief modes.
- Precedence: drag-target highlight > cursor bar > selected > hidden > directory/file (the first two in the order the renderer already uses). Sorting, filtering, selection, the mini-status, Tree mode, Quick view, and git markers are unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `theme-system`: new requirement "`panel.hidden` role" (present in every built-in theme; validation; per-theme values).
- `panel-navigation`: "Entry row rendering" adds hidden styling and precedence.
- `additional-panel-modes`: "Brief display mode" states hidden styling applies in Brief too.

## Impact

- `crates/filecommand-core/src/listing.rs` — `RawDirEntry.hidden`, `Entry.hidden`, `StdFsReader::read_dir` reads the attribute bits (`cfg(windows)`) or the dot prefix.
- `crates/filecommand-core/src/theme.rs` — `Role::PanelHidden` (`"panel.hidden"`), `ALL_ROLES`, six built-in tables.
- `crates/filecommand-tui/src/views/panel.rs` — entry style resolution in Full and Brief.
- Tests: listing attribute mapping (fake reader), theme tables, a snapshot with hidden entries.
- Non-breaking; **note** that `ALL_ROLES` validation means any future user theme file must define `panel.hidden`.
- **Overlap:** `full-mode-column-widths` also carries a MODIFIED "Entry row rendering" delta; whichever lands second re-bases its text on the other's merged requirement.
