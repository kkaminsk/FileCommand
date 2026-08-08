# startup-splash Specification

## Purpose
TBD - created by archiving change m1-shell. Update Purpose after archive.
## Requirements
### Requirement: Frame-1 splash rendering

The system SHALL render the startup splash as the very first painted frame from static identity data — a solid blue backdrop with a horizontally and vertically centered double-line box containing the product name, version, copyright, and tribute lines — so that first paint never waits on any I/O. The identity lines (name, version, copyright, tribute) SHALL be defined in a single place in `core` and used verbatim; the version string SHALL be the crate version. The terminal cursor SHALL be hidden while the splash is shown, and all colors SHALL come from the `splash.*` theme roles.

#### Scenario: Splash is the first frame

- **WHEN** FileCommand launches at or above 80×24 with the splash enabled
- **THEN** the first painted frame is the splash: a solid blue backdrop with a centered double-line box
- **AND** the box contains the product name, `Version <crate-version>`, the copyright line, and the tribute line
- **AND** it paints before the first directory listing has completed

#### Scenario: Splash uses theme roles and hides the cursor

- **WHEN** the splash renders under `nc-classic`
- **THEN** the frame is drawn in `splash.frame` (cyan on blue), the name in `splash.title` (bright-white), the version in `splash.version` (white), and the copyright/tribute in `splash.text` (cyan)
- **AND** the terminal cursor is hidden

#### Scenario: Identity lines are a single source of truth

- **WHEN** the splash renders its identity lines
- **THEN** they are read from the single `core` definition of the identity lines (name, version, copyright, tribute) rather than a splash-local copy

#### Scenario: Mono theme rendering

- **WHEN** the splash renders under `nc-mono`
- **THEN** it renders white on black

### Requirement: Minimum hold and key dismissal

The system SHALL hold the splash for a minimum of 800 ms measured via the injected `Clock`, after which it is replaced by the panels even if the initial listing is still streaming. Any key press SHALL dismiss the splash immediately without waiting for the minimum hold, and the dismissing key event SHALL be consumed — never forwarded to the command line or panels.

#### Scenario: Minimum hold elapses with no input

- **WHEN** no key is pressed and 800 ms have elapsed on the injected `Clock` since the splash first painted
- **THEN** the splash is replaced by the panels
- **AND** the panels render even if the initial directory listing is still streaming

#### Scenario: Key press dismisses before the minimum hold

- **WHEN** a key is pressed 200 ms after the splash first painted
- **THEN** the splash is dismissed immediately, before the 800 ms minimum hold elapses

#### Scenario: Dismissing key is consumed

- **WHEN** a key press dismisses the splash
- **THEN** that key event is consumed and is not forwarded to the command line or the panels

### Requirement: Disabling the splash

The system SHALL skip the splash — making the panels frame 1 — when `splash = false` is set in `config.toml` general options or the `--nosplash` CLI flag is passed. When the flag and config disagree, the `--nosplash` flag SHALL win.

#### Scenario: Disabled via config

- **WHEN** FileCommand launches with `splash = false` in config and no `--nosplash` flag
- **THEN** the splash is skipped and the first painted frame is the panels

#### Scenario: Disabled via flag

- **WHEN** FileCommand launches with the `--nosplash` flag
- **THEN** the splash is skipped and the first painted frame is the panels

#### Scenario: Flag overrides config

- **WHEN** FileCommand launches with `--nosplash` while config has `splash = true`
- **THEN** the flag wins and the splash is skipped

### Requirement: Resize and below-minimum-size behavior

The system SHALL re-center the splash box on terminal resize. If the terminal is below the 80×24 minimum at startup, the splash SHALL be skipped in favor of the "terminal too small" placeholder. If the terminal shrinks below the minimum while the splash is shown, the splash SHALL be replaced by the placeholder and SHALL NOT return when the terminal grows back.

#### Scenario: Box re-centers on resize

- **WHEN** the terminal is resized while the splash is shown and remains at or above 80×24
- **THEN** the splash box is re-centered horizontally and vertically in the new dimensions

#### Scenario: Below minimum at startup

- **WHEN** FileCommand launches with the terminal below 80×24
- **THEN** the splash is skipped and the "terminal too small" placeholder is drawn instead

#### Scenario: Shrinking below minimum mid-splash

- **WHEN** the terminal shrinks below 80×24 while the splash is shown
- **THEN** the splash is replaced by the "terminal too small" placeholder
- **AND** the splash does not return when the terminal is enlarged back to at or above 80×24

