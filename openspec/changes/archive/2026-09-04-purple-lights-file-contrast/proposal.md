# Change: purple-lights-file-contrast

## Why

In `purple-lights`, file rows are the least readable text on screen: bright-magenta on magenta in ANSI-16, and orchid (`#BA55D3`) on deep purple (`#300040`) in the truecolor rendition — purple-on-purple in both cases, with file names (the panel's primary content) carrying less contrast than the frame around them. The user has asked for a dark grey file foreground to make file rows readable. Directories, the cursor, selection, and git-status colors already carry distinct, higher-contrast colors and are unaffected.

## What Changes

- `purple-lights`' `panel.file` role changes foreground: ANSI-16 `bright-black` (the 16-color palette's dark grey) on the unchanged magenta base, and truecolor `#A9A9A9` (dark grey) on the unchanged `#300040` deep-purple backdrop.
- No other role in any theme changes; the mini-status, directory, cursor, selected, and git-status roles keep their current colors.

## Capabilities

### New Capabilities

*(none)*

### Modified Capabilities

- `theme-system`: one ADDED requirement pinning `purple-lights`' file-row colors for readability. (The existing purple-lights requirement anchors frame/directory/cursor/keybar only — it never anchored `panel.file` — so this adds an anchor rather than contradicting one.)

## Impact

- **Crates:** `filecommand-core` — `theme.rs` (`purple_lights()`: the `Role::PanelFile` ANSI entry and its truecolor override). `filecommand-tui` — refresh of any snapshot rendering purple-lights panels.
- **Depends on:** `visual-themes` (the purple-lights theme).
- **Out of scope:** contrast changes to other purple-lights roles or other themes; configurable per-role overrides.
