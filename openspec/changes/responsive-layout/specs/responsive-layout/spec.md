# responsive-layout Specification (delta)

## ADDED Requirements

### Requirement: Degraded band and per-panel breakpoints

Between the 60×16 hard floor and the 80×24 nominal size, the system SHALL keep every feature available while degrading rendering per the rules of this capability; no surface may disappear, silently truncate mid-label, or paint outside the terminal. All panel-content degradation decisions (Full-mode column ladder, Brief column count, mini-status fields) SHALL be functions of the individual panel's width, not the terminal's, and degradation SHALL be deterministic and reversible: growing a surface back restores exactly the richer rendering it had at that size before.

#### Scenario: All features remain available in the degraded band

- **WHEN** the terminal is 70×20
- **THEN** the panels, command line, and F-key bar all render and every command that works at 80×24 remains invocable

#### Scenario: Degradation keys off the panel, not the terminal

- **WHEN** the terminal is 100 columns wide with the split adjusted so the left panel is 24 columns and the right panel is 76 columns
- **THEN** the left panel renders with a reduced column set per its own width while the right panel renders all four Full-mode columns

#### Scenario: Degradation is reversible

- **WHEN** a panel shrinks so Full mode drops the Time column and is later restored to its prior width
- **THEN** the Time column returns and the panel renders exactly as it did at that width before

---

### Requirement: Unified overlay geometry

The system SHALL size and position every overlay — startup splash, Help window, About dialog, operation/input/confirmation/error/progress dialogs, drive select, find-file, fuzzy jump, user menu, quit dialog, and F9 pull-down boxes — through a single core-owned geometry rule: given the overlay's preferred size and minimum size, each dimension is `clamp(min(preferred, terminal − 2), minimum, terminal)` and the result is centered in the terminal. Every overlay's declared minimum SHALL be at most 58×14 so all overlays are renderable at the 60×16 floor. Overlay interiors SHALL truncate content with `…` rather than paint outside the computed rectangle, and every open overlay SHALL be repositioned by this rule on every terminal resize. F9 pull-down boxes SHALL apply the same size clamping but SHALL shift left only as far as needed to stay fully on-screen, remaining anchored under their menu title, instead of centering.

#### Scenario: Overlay at nominal size uses its preferred geometry

- **WHEN** an overlay with preferred size 52×10 opens at terminal size 80×24
- **THEN** it renders exactly 52×10, centered

#### Scenario: Overlay clamps near the floor

- **WHEN** an overlay with preferred size 62×19 opens at terminal size 60×16
- **THEN** it renders at 58×14 — clamped to terminal minus the 2-cell margin — centered, fully on-screen, with interior content truncated with `…` where it no longer fits

#### Scenario: Overlays re-center on resize

- **WHEN** any overlay is open and the terminal is resized within the supported range
- **THEN** the overlay's rectangle is recomputed by the geometry rule and re-centered in the new dimensions

#### Scenario: Pull-down box shifts left instead of centering

- **WHEN** the Right menu's pull-down would extend past the terminal's right edge
- **THEN** the box shifts left just far enough to fit fully on-screen and remains attached below the menu bar

---

### Requirement: F-key bar degradation forms

The system SHALL render the F-key bar in the widest of three canonical forms that fits the terminal width, never truncating a slot mid-label: the full form `1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete 9PullDn 10Quit` (67 columns), the short form `1Hlp 2Mnu 3Vew 4Edt 5Cpy 6Ren 7Mkd 8Del 9Pdn 10Qit` (50 columns), and the numbers-only form `1 2 3 4 5 6 7 8 9 10` (20 columns). All ten slots SHALL always be present in whichever form renders. The same widest-form-that-fits rule SHALL govern the Ctrl and Alt modifier label variants and the viewer and editor F-key bars, each with its own short-form labels of the same shape.

#### Scenario: Full form at nominal width

- **WHEN** the terminal is 80 columns wide
- **THEN** the key bar renders the full form with all ten labeled slots

#### Scenario: Short form at the floor

- **WHEN** the terminal is 60 columns wide
- **THEN** the key bar renders the short form `1Hlp 2Mnu 3Vew 4Edt 5Cpy 6Ren 7Mkd 8Del 9Pdn 10Qit` with all ten slots visible

#### Scenario: No mid-label truncation

- **WHEN** the terminal width falls between two forms' required widths
- **THEN** the narrower form renders in full; no slot is ever partially drawn

---

### Requirement: Full-screen surface degradation

Within the viewer and editor, the body SHALL reflow to the terminal size, and the header row SHALL drop its indicators right-to-left as width runs out — file size and `Ovr` flag first, then the `Line/Col` indicator — keeping the file path visible last, truncated from the left with a leading `…` when even the path alone does not fit. The viewer and editor F-key bars SHALL follow the F-key bar degradation forms.

#### Scenario: Editor header drops indicators on a narrow terminal

- **WHEN** the editor is open on a terminal too narrow to show path, `Line/Col`, and size/`Ovr` together
- **THEN** size and `Ovr` disappear first, then `Line/Col`, and the path remains, left-truncated with `…` if necessary

#### Scenario: Viewer stays functional at the floor

- **WHEN** the viewer is open at terminal size 60×16
- **THEN** the content body reflows to the available area and the viewer key bar renders a complete canonical form

---

### Requirement: Chrome degradation

The clock SHALL render over the right panel's top border only when it fits without touching the centered path title; otherwise it SHALL be hidden entirely and never partially drawn. The command line SHALL scroll horizontally to keep the caret visible, and when the prompt alone exceeds half the terminal width the prompt SHALL truncate from the left with a leading `…`. The panel mini-status SHALL drop the same fields in the same order as the Full-mode column ladder (Time, then Date, then Size), truncating the entry name last with `…`; the `N files selected, X bytes` summary SHALL truncate with `…` when it does not fit.

#### Scenario: Clock hides rather than colliding

- **WHEN** the right panel's top border is too narrow for the clock and the centered path title to both fit
- **THEN** the clock is not drawn at all and the path title renders normally

#### Scenario: Command line keeps the caret visible

- **WHEN** the typed command exceeds the visible command-line width
- **THEN** the visible window scrolls so the caret remains on-screen

#### Scenario: Mini-status drops fields in ladder order

- **WHEN** a panel is narrow enough that its mini-status cannot show name, size, date, and time
- **THEN** time is dropped first, then date, then size, and the name truncates with `…` only after all other fields are gone
