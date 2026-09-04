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

