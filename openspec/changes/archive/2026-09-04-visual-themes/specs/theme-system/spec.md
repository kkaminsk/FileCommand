# theme-system Specification (delta)

## ADDED Requirements

### Requirement: Built-in terminal-green theme

The system SHALL ship a compiled-in theme named `terminal-green` rendered as green-phosphor monochrome: base text green on black, directories and selected entries bright-green, frames/gauges/`…` green, and every role that is inverse in `nc-classic` (cursor, active title, key-bar labels, clock, active tab, menus, dialogs, viewer/editor headers, buttons) black on green. Roles MAY carry optional truecolor phosphor-green overrides. No color in `terminal-green` SHALL carry meaning that is not also carried by case, position, or inversion.

#### Scenario: Green base and bright-green directories

- **WHEN** `terminal-green` is active and a normal file entry and a directory entry are drawn
- **THEN** the file renders green on black and the directory renders bright-green on black

#### Scenario: Inversions render black on green

- **WHEN** `terminal-green` is active and the panel cursor bar and key-bar labels are drawn
- **THEN** each renders black on green

---

### Requirement: Built-in purple-lights theme

The system SHALL ship a compiled-in theme named `purple-lights` that follows the `nc-classic` role structure with magenta standing in for blue and bright-magenta for cyan: `screen.backdrop` background magenta, `panel.frame` bright-magenta on magenta, `panel.directory` bright-white on magenta, `panel.cursor` black on bright-magenta, and `keybar.label` black on bright-magenta. Roles MAY carry optional truecolor overrides toward a deep-purple/violet rendition on truecolor terminals.

#### Scenario: purple-lights role anchors

- **WHEN** `purple-lights` is active
- **THEN** the screen backdrop is magenta, panel frames render bright-magenta on magenta, directories render bright-white on magenta, and the panel cursor renders black on bright-magenta

#### Scenario: Truecolor terminals get the violet rendition

- **WHEN** `purple-lights` roles carry `#RRGGBB` overrides and the terminal reports truecolor support
- **THEN** the truecolor values are emitted, while non-truecolor terminals render the mandatory ANSI-16 magenta palette

---

### Requirement: Built-in yellow-storm theme

The system SHALL ship a compiled-in theme named `yellow-storm` rendered as an amber terminal: base text yellow on black, directories bright-yellow, selected entries bright-white (preserving the selection distinction that `nc-classic` carries with yellow), frames/gauges yellow, and every role that is inverse in `nc-classic` black on yellow. Roles MAY carry optional truecolor amber overrides. No color in `yellow-storm` SHALL carry meaning that is not also carried by case, position, or inversion.

#### Scenario: Amber base and bright-yellow directories

- **WHEN** `yellow-storm` is active and a normal file entry and a directory entry are drawn
- **THEN** the file renders yellow on black and the directory renders bright-yellow on black

#### Scenario: Selected entries remain distinguishable

- **WHEN** `yellow-storm` is active and a selected entry is drawn
- **THEN** it renders bright-white on black, distinct from both normal (yellow) and directory (bright-yellow) entries

#### Scenario: Inversions render black on yellow

- **WHEN** `yellow-storm` is active and the panel cursor bar and dialog bodies are drawn
- **THEN** each renders black on yellow

---

### Requirement: Built-in inverted high-contrast theme

The system SHALL ship a compiled-in theme named `inverted` as a high-contrast black-on-white accessibility theme for vision-impaired users — the light counterpart of `nc-mono`. Base text SHALL render black on bright-white, every role that is inverse in `nc-classic` (cursor, active title, key-bar labels, clock, active tab, menus, dialogs, viewer/editor headers, buttons) SHALL render bright-white on black, and directories and selected entries SHALL remain distinguishable without hue. The theme SHALL use only black, white, and bright-white — no hue anywhere — and no color SHALL carry meaning that is not also carried by case, position, or inversion.

#### Scenario: High-contrast base is black on bright-white

- **WHEN** `inverted` is active and a normal file entry is drawn
- **THEN** it renders black on bright-white

#### Scenario: Inversions render bright-white on black

- **WHEN** `inverted` is active and the panel cursor bar, key-bar labels, and an open dialog are drawn
- **THEN** each renders bright-white on black

#### Scenario: No hue is emitted

- **WHEN** any screen is rendered under `inverted`
- **THEN** every emitted color is black, white, or bright-white
- **AND** directories and selected entries remain distinguishable from normal entries by case, position, or inversion

---

### Requirement: New themes satisfy validation and swap semantics

Each of `terminal-green`, `purple-lights`, `yellow-storm`, and `inverted` SHALL define every role required by any renderer (passing the existing every-role-defined validation), SHALL carry mandatory ANSI-16 named values for every role per the color-depth policy, and SHALL be switchable to and from at runtime with no renderer changes, exactly as the existing `nc-classic`/`nc-mono` swap behaves.

#### Scenario: New themes pass role validation

- **WHEN** each new built-in theme is loaded
- **THEN** every role referenced by any renderer resolves to a defined color specification with a mandatory ANSI-16 named value

#### Scenario: Runtime swap to a new theme restyles everything

- **WHEN** the active theme is switched from `nc-classic` to any of the four new themes
- **THEN** the same renderers produce output using the new theme's role table
- **AND** no renderer contains a color branch specific to any theme
