# theme-system Specification

## Purpose
TBD - created by archiving change m1-shell. Update Purpose after archive.
## Requirements
### Requirement: Named role-to-color theme model

The system SHALL model a theme as a map from a fixed set of named rendering roles (e.g. `panel.frame`, `panel.directory`, `panel.cursor`, `keybar.label`, `dialog.primary`, `splash.title`, `screen.placeholder`) to a color specification, and every renderer SHALL obtain its foreground/background colors by looking up a role in the active theme rather than hardcoding any color. The theme SHALL be the single styling authority for all rendering.

#### Scenario: Renderer resolves color through a role

- **WHEN** a panel entry that is a directory is drawn under the `nc-classic` theme
- **THEN** its style is resolved by looking up the `panel.directory` role in the active theme
- **AND** no color value is emitted that was not obtained from a theme role lookup

#### Scenario: Every role required by renderers is defined

- **WHEN** a built-in theme is loaded
- **THEN** every role referenced by any M1 renderer (panel frame/title/header/file/directory/cursor/selected/ministatus, keybar number/label, commandline, clock, dialog roles used by the quit-confirm, splash roles, and `screen.placeholder`) resolves to a defined color specification
- **AND** no renderer encounters an undefined role at render time

#### Scenario: Swapping the active theme restyles without renderer changes

- **WHEN** the active theme is switched from `nc-classic` to `nc-mono`
- **THEN** the same renderers produce output using the new theme's role table
- **AND** no renderer contains a color branch specific to either theme

### Requirement: ANSI-16 named color depth policy

Every role in every theme MUST specify a color using an ANSI-16 named color drawn from `black, red, green, yellow, blue, magenta, cyan, white` and their `bright-` variants, and this named value is mandatory as the fallback for every role. A role MAY additionally carry a `#RRGGBB` truecolor value; the truecolor value SHALL be used only when the terminal reports truecolor support, otherwise the mandatory ANSI-16 named value SHALL render. Rendering SHALL emit standard 16-color attributes for named colors and SHALL NOT use 256-color indexed colors.

#### Scenario: Named color is mandatory for every role

- **WHEN** any built-in or user theme is validated
- **THEN** every role has an ANSI-16 named foreground and background (or the sentinel "none"/inherit where the role table specifies `—`)
- **AND** a theme missing an ANSI-16 value for any role is rejected as invalid

#### Scenario: Truecolor used only when the terminal supports it

- **WHEN** a role carries a `#RRGGBB` value and the terminal reports truecolor support
- **THEN** the `#RRGGBB` value is emitted for that role

#### Scenario: Truecolor absent or unsupported falls back to the named color

- **WHEN** a role carries a `#RRGGBB` value but the terminal does not report truecolor support, or the role carries no `#RRGGBB` value
- **THEN** the role's mandatory ANSI-16 named color is emitted

#### Scenario: No 256-color indexed output

- **WHEN** any role is rendered
- **THEN** the emitted attribute is either a standard 16-color named attribute or a truecolor attribute
- **AND** no 256-color indexed palette attribute is ever emitted

### Requirement: CP437-only iconography

Rendering SHALL use only ASCII plus the CP437-heritage box-drawing and geometric glyph set (`═ ║ ╔ ╗ ╚ ╝ ─ │ ┌ ┐ └ ┘ ├ ┤ ▶ ◀ ↑ ↓ ◄ ► █ ░ …`), all of which are single-cell in `unicode-width`. The system MUST NOT emit Nerd Font glyphs, emoji, or file-type icons; file-type differentiation is by color and case only.

#### Scenario: No Nerd Fonts, emoji, or icons in output

- **WHEN** any M1 screen is rendered (panels, splash, F-key bar, mini-status, placeholder)
- **THEN** every glyph emitted is either ASCII or a member of the permitted CP437 box-drawing/geometric set
- **AND** no Nerd Font private-use glyph, emoji, or file-type icon appears

#### Scenario: File type differentiated by color and case, not icons

- **WHEN** a directory and a file are drawn in the same panel
- **THEN** they are distinguished by their role colors and letter case (directories uppercase/bright-white, files per the file role)
- **AND** neither entry is preceded by a type icon glyph

#### Scenario: Permitted glyphs remain single-cell

- **WHEN** a box-drawing or geometric glyph from the permitted set is measured with `unicode-width`
- **THEN** its display width is exactly one cell, preserving column alignment

### Requirement: Built-in nc-classic default theme

The system SHALL ship a compiled-in theme named `nc-classic` that is the default when no theme is configured, and its role table SHALL match the normative `nc-classic` table (e.g. `screen.backdrop` background blue; `panel.frame` cyan on blue; `panel.directory` bright-white on blue; `panel.cursor` black on cyan; `keybar.number` white on black; `keybar.label` black on cyan; `splash.title` bright-white on blue; `splash.frame` cyan on blue; `screen.placeholder` white on blue).

#### Scenario: nc-classic is the default theme

- **WHEN** the application starts with no `theme` value configured
- **THEN** the active theme is `nc-classic`

#### Scenario: nc-classic role colors match the normative table

- **WHEN** `nc-classic` is active
- **THEN** `panel.directory` renders bright-white on blue, `panel.cursor` renders black on cyan, and `keybar.label` renders black on cyan
- **AND** `splash.title` renders bright-white on blue while `splash.frame` renders cyan on blue

### Requirement: Built-in nc-mono theme

The system SHALL ship a compiled-in theme named `nc-mono` rendered entirely in white on black, in which every role that is inverse in `nc-classic` (cursor, active title, key-bar labels, clock, active tab, menus, dialogs, viewer/editor headers, buttons) becomes black on white, directories and selected entries render bright-white, and frames/`…`/gauges render white. No color in `nc-mono` SHALL carry meaning that is not also carried by case, position, or inversion.

#### Scenario: nc-mono base is white on black

- **WHEN** `nc-mono` is active and a normal file entry is drawn
- **THEN** it renders white on black

#### Scenario: nc-classic inversions become black on white under nc-mono

- **WHEN** `nc-mono` is active and the panel cursor bar and active panel title are drawn
- **THEN** each renders black on white (the inversion of the white-on-black base)

#### Scenario: Directories and selected entries stay bright-white

- **WHEN** `nc-mono` is active and a directory entry and a selected entry are drawn
- **THEN** both render bright-white on black
- **AND** the distinction from normal entries is preserved without relying on hue

