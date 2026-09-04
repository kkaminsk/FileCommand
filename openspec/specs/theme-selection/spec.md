# theme-selection Specification

## Purpose
TBD - created by archiving change visual-themes. Update Purpose after archive.
## Requirements
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

### Requirement: Launch-time theme override via --theme

The system SHALL accept a command-line switch `--theme <name>` (equivalently `--theme=<name>`) naming a built-in theme, and SHALL start the session with that theme active, taking precedence over the `theme` key in `config.toml`. The override SHALL be session-only: it SHALL NOT write configuration, and applying a theme from a picker during the session SHALL persist exactly as without the switch. An unknown name, or a `--theme` switch with no value, SHALL NOT prevent launch: the session SHALL start with the configured theme (falling back to the default as already specified) and SHALL raise the dismissable startup-warning dialog naming the rejected value and listing the valid built-in theme names.

#### Scenario: Valid override wins over configuration

- **WHEN** `config.toml` contains `theme = "nc-classic"` and the application is launched with `--theme yellow-storm`
- **THEN** the session starts with `yellow-storm` active and `config.toml` is not modified

#### Scenario: Override is session-only

- **WHEN** the application is launched with `--theme terminal-green`, the user applies `purple-lights` from a picker, and the application is later relaunched without the switch
- **THEN** `config.toml` contains `theme = "purple-lights"` and the relaunched session starts with `purple-lights` active

#### Scenario: Unknown name warns and falls back

- **WHEN** the application is launched with `--theme no-such-theme` and `config.toml` contains `theme = "nc-mono"`
- **THEN** the session starts with `nc-mono` active
- **AND** the dismissable startup-warning dialog is shown naming `no-such-theme` and listing the valid built-in theme names

#### Scenario: Missing value warns and falls back

- **WHEN** the application is launched with `--theme` as the final argument
- **THEN** the session starts with the configured theme and the startup-warning dialog reports the missing value

### Requirement: Live theme preview while the picker is open

While the theme picker is open, the system SHALL render every surface of the frame — panels, key bar, command line, clock, the picker dialog itself, and any overlay drawn above it — by resolving roles through the currently highlighted theme's role table, updating on the next frame after every highlight move. The preview SHALL be render-only: the active theme, the persisted configuration, and the picker's active-theme marker SHALL continue to reflect the applied theme until Enter is pressed. Closing the picker without applying SHALL restore rendering through the applied theme.

#### Scenario: Moving the highlight previews the theme

- **WHEN** the active theme is `nc-classic`, the picker is open, and the user moves the highlight to `purple-lights`
- **THEN** the next frame renders every element by resolving roles in the `purple-lights` table
- **AND** the active theme remains `nc-classic` and `config.toml` is not written

#### Scenario: Esc restores the applied theme's rendering

- **WHEN** the picker is open with `yellow-storm` highlighted and previewed, and the user presses Esc
- **THEN** the dialog closes and the next frame renders entirely through the applied theme's role table

#### Scenario: The marker stays on the applied theme during preview

- **WHEN** the active theme is `nc-classic` and the highlight is on `inverted`
- **THEN** the frame renders in `inverted` while the picker's active-theme marker remains on the `nc-classic` row

#### Scenario: Opening the picker changes nothing visually

- **WHEN** the picker opens with the highlight on the active theme
- **THEN** the rendered frame is identical to the frame before the picker opened, apart from the picker dialog itself

