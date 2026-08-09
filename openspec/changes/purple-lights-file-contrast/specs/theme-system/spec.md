# theme-system Specification (delta)

## ADDED Requirements

### Requirement: purple-lights file rows render in dark grey

In the `purple-lights` theme, the `panel.file` role SHALL render with a dark grey foreground for readability: the mandatory ANSI-16 value SHALL be `bright-black` on the theme's magenta base, and the truecolor override SHALL be `#A9A9A9` on the theme's `#300040` backdrop. Directory, cursor, selected, mini-status, and git-status roles SHALL be unaffected.

#### Scenario: File rows are dark grey in the truecolor rendition

- **WHEN** `purple-lights` is active on a truecolor terminal and a normal file entry is drawn
- **THEN** the file name renders `#A9A9A9` on `#300040`

#### Scenario: ANSI-16 fallback uses the palette's dark grey

- **WHEN** `purple-lights` is active on a non-truecolor terminal and a normal file entry is drawn
- **THEN** the file name renders bright-black on magenta

#### Scenario: Neighbouring roles are unchanged

- **WHEN** `purple-lights` is active and a directory entry, the cursor row, and a selected entry are drawn
- **THEN** they render with the same colors as before this change
