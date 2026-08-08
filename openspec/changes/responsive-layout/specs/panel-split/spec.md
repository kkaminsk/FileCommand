# panel-split Specification (delta)

## ADDED Requirements

### Requirement: Adjust and reset the panel split

The system SHALL move the vertical divider between the panels 2 columns left on Ctrl+Left and 2 columns right on Ctrl+Right, and SHALL reset the split to 50/50 on Ctrl+=. All three bindings SHALL be overridable in `config.toml` per the existing keymap convention. An adjustment that would reduce either panel below its minimum width SHALL be a no-op. Split state SHALL live in `filecommand-core` with all mutations flowing through `core::update`.

#### Scenario: Divider moves in 2-column steps

- **WHEN** the split is at 50/50 on a 100-column terminal and the user presses Ctrl+Right
- **THEN** the divider moves 2 columns right, widening the left panel to 52 columns and narrowing the right panel to 48

#### Scenario: Adjustment at the limit is a no-op

- **WHEN** the right panel is at its 20-column minimum and the user presses Ctrl+Right
- **THEN** nothing changes — the divider stays where it is

#### Scenario: Reset restores 50/50

- **WHEN** the split has been adjusted away from 50/50 and the user presses Ctrl+=
- **THEN** the split returns to 50/50

---

### Requirement: Split ratio semantics and panel minimum

The system SHALL store the split as an integer left-panel percentage (`split_percent`, default 50) and derive the effective left-panel width as `round(terminal_width × percent / 100)` using round-half-up, then clamp the result at layout time so each panel keeps at least 20 columns. Clamping SHALL be non-destructive: a terminal resize never rewrites the stored percentage, so a split that cannot be honored at the current size renders clamped and is honored again when the terminal grows.

#### Scenario: Percentage scales across resizes

- **WHEN** the split is 60% and the terminal is resized from 100 to 160 columns
- **THEN** the left panel is 96 columns — the same 60% of the new width

#### Scenario: Clamping preserves the stored intent

- **WHEN** the split is 75% and the terminal shrinks to 60 columns
- **THEN** the right panel is held at its 20-column minimum while the terminal is small
- **AND** enlarging the terminal back to 120 columns restores a 90-column left panel — the stored 75% unchanged

---

### Requirement: Split persistence to configuration

When the split is adjusted or reset, the system SHALL persist `panel_split = <percent>` to `config.toml` using the same atomic temp-file-and-rename write as other configuration updates; rapid repeated adjustments MAY coalesce writes, but the final value SHALL be persisted. On startup the configured value SHALL be loaded; an unset, non-integer, or out-of-range value SHALL fall back to 50 without error.

#### Scenario: Split survives restart

- **WHEN** the user adjusts the split to 66% and later restarts the application
- **THEN** `config.toml` contains `panel_split = 66` and the panels open at that split

#### Scenario: Invalid configured value falls back

- **WHEN** `config.toml` contains `panel_split = "wide"` and the application starts
- **THEN** the split is 50/50 and the application runs normally

#### Scenario: Config write is atomic

- **WHEN** a split adjustment triggers the `config.toml` update
- **THEN** the file is replaced atomically so a crash mid-write cannot leave a truncated configuration
