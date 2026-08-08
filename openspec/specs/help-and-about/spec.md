# help-and-about Specification

## Purpose
TBD - created by archiving change m5-editor-and-modern-extras. Update Purpose after archive.
## Requirements
### Requirement: F1 Help window frame and identity header

The system SHALL open a Help window when F1 is pressed, rendered as a centered primary-style dialog (black text on cyan, black double-line frame) with the title `Help` set into the top border, and it SHALL begin with a header block of three centered black-on-cyan lines carrying the identity lines (name + version, copyright, tribute) — the exact same strings shared verbatim with the startup splash and the Info-panel version banner.

#### Scenario: F1 opens the centered Help window in primary style

- **WHEN** the user presses F1 from the panels at terminal size 80×24
- **THEN** a centered window (approximately 62×19, capped near that proportion) is drawn in the primary dialog style (black on cyan, black double-line frame) with the title `Help` set into its top border

#### Scenario: Identity header matches the shared source of truth

- **WHEN** the Help window is open
- **THEN** its header block shows three centered black-on-cyan lines — the product name with version, the copyright line, and the tribute line — byte-for-byte identical to the splash identity lines and the Info-panel version banner

#### Scenario: Help window re-centers on resize

- **WHEN** the terminal is resized while the Help window is open
- **THEN** the window re-centers within the new dimensions, scaling toward its capped proportion, and remains fully within the visible area

### Requirement: Help topic list

The system SHALL render, below the identity header, a scrollable list of help topics whose first entry is `About FileCommand`, highlighted (white on black) as the initial cursor position, followed by the v1 topics `Keyboard reference`, `Panels and display modes`, `File operations`, `Menus`, `Viewer`, `Editor`, `Command line`, `Modern extras`, and `Configuration`, and it SHALL show `↑` / `↓` scroll arrows on the right border only when the list overflows the visible area.

#### Scenario: List opens with About FileCommand highlighted first

- **WHEN** the Help window first appears
- **THEN** the topic list is shown with `About FileCommand` as the first entry and the cursor highlight (white on black) resting on it

#### Scenario: Cursor moves through the topic list

- **WHEN** the user presses Down arrow one or more times within the topic list
- **THEN** the white-on-black highlight moves to the next topic and the list scrolls as needed to keep the highlighted topic visible

#### Scenario: Scroll arrows appear only on overflow

- **WHEN** the topic list is taller than the visible list area
- **THEN** `↑` / `↓` scroll arrows render on the window's right border, and they are absent when the entire list fits

### Requirement: Help window buttons

The system SHALL present two buttons in the Help window — `Help` as the default (black on bright-yellow), which opens the highlighted topic exactly as Enter does, and `Cancel` (black on white), which closes the window exactly as Esc does.

#### Scenario: Help button opens the highlighted topic

- **WHEN** the user activates the `Help` button (or presses Enter) while a non-About topic is highlighted
- **THEN** that topic's page opens in place of the list

#### Scenario: Cancel button closes the window

- **WHEN** the user activates the `Cancel` button (or presses Esc) from the topic list
- **THEN** the Help window closes and focus returns to the panels

### Requirement: Help topic pages

The system SHALL replace the topic list with the selected topic's page inside the same Help window, rendering static text compiled into the binary, and Esc SHALL return from a topic page to the topic list rather than closing the window; the `Keyboard reference` page SHALL document the Ctrl/Alt F-key-bar variants.

#### Scenario: Selecting a topic replaces the list with its page

- **WHEN** the user opens the `Menus` topic from the list
- **THEN** the topic list is replaced within the same window by the `Menus` page rendered from compiled-in static text, with no filesystem read performed

#### Scenario: Esc returns from a topic page to the list

- **WHEN** the user presses Esc while viewing a topic page
- **THEN** the window returns to the topic list with the previously highlighted topic still selected, and the window remains open

#### Scenario: Keyboard reference documents the modifier bar variants

- **WHEN** the user opens the `Keyboard reference` topic
- **THEN** its page includes documentation of the Ctrl and Alt F-key-bar label variants

### Requirement: About FileCommand dialog

The system SHALL open a secondary-style About dialog (grey style: black text on white, black single-line frame), centered and roughly 52×10, when `About FileCommand` is activated from the Help topic list, and it SHALL display the identity lines plus a `License: MIT OR Apache-2.0` line and the repository URL, with a single `OK` button that dismisses the dialog.

#### Scenario: Enter on About FileCommand opens the secondary-style dialog

- **WHEN** the user presses Enter (or activates `Help`) with `About FileCommand` highlighted
- **THEN** a centered secondary-style dialog (black on white, black single-line frame, roughly 52×10) opens over the Help window

#### Scenario: About dialog shows identity, license, and repository

- **WHEN** the About dialog is open
- **THEN** it shows the shared identity lines, a line reading `License: MIT OR Apache-2.0`, and the repository URL, and a single `OK` button

#### Scenario: OK dismisses the About dialog

- **WHEN** the user activates `OK` (or presses Esc) in the About dialog
- **THEN** the About dialog closes and the Help topic list is shown again with `About FileCommand` still highlighted

