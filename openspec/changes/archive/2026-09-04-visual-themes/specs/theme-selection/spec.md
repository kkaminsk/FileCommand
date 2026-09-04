# theme-selection Specification (delta)

## ADDED Requirements

### Requirement: Options menu opens the theme picker

The system SHALL open a modal theme-picker dialog when the user activates Options → Themes in the F9 pull-down menus. The dialog SHALL render in the primary dialog style (§4.4) using only theme roles and CP437-heritage glyphs, listing every built-in theme by name with the currently active theme marked. The dialog SHALL open with the active theme highlighted. Picker state SHALL live in `filecommand-core` with all mutations flowing through `core::update`.

#### Scenario: Themes item opens the picker

- **WHEN** the user opens the F9 Options menu and activates Themes
- **THEN** the menu overlay closes and a modal theme-picker dialog opens listing `nc-classic`, `nc-mono`, `terminal-green`, `purple-lights`, `yellow-storm`, and `inverted`

#### Scenario: Active theme is marked and pre-highlighted

- **WHEN** the active theme is `terminal-green` and the picker opens
- **THEN** the `terminal-green` row carries the active-theme marker and is the highlighted row

---

### Requirement: Picker navigation, apply, and cancel

Within the theme picker, Up/Down SHALL move the highlight over the theme list, Enter SHALL apply the highlighted theme and close the dialog, and Esc SHALL close the dialog leaving the active theme and configuration unchanged. Applying a theme SHALL take effect immediately: the entire screen — panels, key bar, command line, menus, dialogs, clock — SHALL render with the new theme's role table on the next frame, with no renderer changes and no restart.

#### Scenario: Enter applies the highlighted theme immediately

- **WHEN** the picker is open, the user highlights `yellow-storm`, and presses Enter
- **THEN** the dialog closes and the next rendered frame draws every element by resolving roles in the `yellow-storm` table

#### Scenario: Esc changes nothing

- **WHEN** the active theme is `nc-classic`, the picker is open with `purple-lights` highlighted, and the user presses Esc
- **THEN** the dialog closes, the active theme remains `nc-classic`, and `config.toml` is not written

---

### Requirement: Applied theme persists to configuration

When a theme is applied from the picker, the system SHALL write `theme = "<name>"` to `config.toml` using an atomic write, so the choice survives restart. On startup the configured theme SHALL be loaded as the active theme; an unset or unknown `theme` value SHALL fall back to `nc-classic` without error.

#### Scenario: Selection survives restart

- **WHEN** the user applies `purple-lights` from the picker and later restarts the application
- **THEN** `config.toml` contains `theme = "purple-lights"` and the application starts with `purple-lights` active

#### Scenario: Unknown configured theme falls back to default

- **WHEN** `config.toml` contains `theme = "no-such-theme"` and the application starts
- **THEN** the active theme is `nc-classic` and the application runs normally

#### Scenario: Config write is atomic

- **WHEN** applying a theme triggers the `config.toml` update
- **THEN** the file is replaced atomically so a crash mid-write cannot leave a truncated configuration
