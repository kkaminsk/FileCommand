# help-and-about Specification (delta)

## MODIFIED Requirements

### Requirement: F1 Help window frame and identity header

The system SHALL open a Help window when F1 is pressed, rendered as a centered primary-style dialog (black text on cyan, black double-line frame) with the title `Help` set into the top border, and it SHALL begin with a header block of three centered black-on-cyan lines carrying the identity lines (name + version, copyright, tribute) — the exact same strings shared verbatim with the startup splash and the Info-panel version banner. The window's geometry SHALL come from the unified overlay geometry rule (`responsive-layout`) with a preferred size of 62×19 and a minimum of 40×10, so it renders 62×19 at the 80×24 nominal size and clamps smaller terminals down to the 60×16 floor.

#### Scenario: F1 opens the centered Help window in primary style

- **WHEN** the user presses F1 from the panels at terminal size 80×24
- **THEN** a centered window of 62×19 is drawn in the primary dialog style (black on cyan, black double-line frame) with the title `Help` set into its top border

#### Scenario: Help window clamps below the nominal size

- **WHEN** the user presses F1 at terminal size 60×16
- **THEN** the Help window renders centered at 58×14 — clamped by the unified overlay rule — fully on-screen, with its content truncated with `…` where it no longer fits

#### Scenario: Identity header matches the shared source of truth

- **WHEN** the Help window is open
- **THEN** its header block shows three centered black-on-cyan lines — the product name with version, the copyright line, and the tribute line — byte-for-byte identical to the splash identity lines and the Info-panel version banner

#### Scenario: Help window re-centers on resize

- **WHEN** the terminal is resized while the Help window is open
- **THEN** the window's rectangle is recomputed by the unified overlay rule, re-centered within the new dimensions, and remains fully within the visible area
