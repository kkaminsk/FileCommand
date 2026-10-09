## ADDED Requirements

### Requirement: `panel.hidden` role

The theme model SHALL include a `panel.hidden` role used for entries flagged hidden or system. Every built-in theme SHALL define it with a mandatory ANSI-16 pair that is visibly dimmer than, and distinct from, that theme's `panel.file` role on the same background, with an optional truecolor override; a theme lacking the role SHALL fail validation like any other missing role.

#### Scenario: Built-in themes define the role
- **WHEN** any built-in theme (`nc-classic`, `nc-mono`, `terminal-green`, `purple-lights`, `yellow-storm`, `inverted`) is loaded
- **THEN** `panel.hidden` resolves to a defined color pair that differs from the theme's `panel.file` pair

#### Scenario: nc-classic hidden color
- **WHEN** `nc-classic` is active and a hidden file entry is drawn
- **THEN** its name renders bright-black on blue

#### Scenario: Missing role is rejected
- **WHEN** a theme table omits `panel.hidden`
- **THEN** theme validation fails naming `panel.hidden`
