# theme-selection Specification (delta)

## ADDED Requirements

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
