## MODIFIED Requirements

### Requirement: Degraded band and per-panel breakpoints

Between the 60×16 hard floor and the 80×24 nominal size, the system SHALL keep every feature available while degrading rendering per the rules of this capability; no surface may disappear, silently truncate mid-label, or paint outside the terminal. All panel-content degradation decisions (Full-mode column ladder, Brief column count, mini-status fields) SHALL be functions of the individual panel's width, not the terminal's, and degradation SHALL be deterministic and reversible: growing a surface back restores exactly the richer rendering it had at that size before.

#### Scenario: All features remain available in the degraded band

- **WHEN** the terminal is 70×20
- **THEN** the panels, path boxes, and F-key bar all render and every command that works at 80×24 remains invocable

#### Scenario: Degradation keys off the panel, not the terminal

- **WHEN** the terminal is 100 columns wide with the split adjusted so the left panel is 24 columns and the right panel is 76 columns
- **THEN** the left panel renders with a reduced column set per its own width while the right panel renders all four Full-mode columns

#### Scenario: Degradation is reversible

- **WHEN** a panel shrinks so Full mode drops the Time column and is later restored to its prior width
- **THEN** the Time column returns and the panel renders exactly as it did at that width before

### Requirement: Chrome degradation

The clock SHALL render over the right panel's top border only when it fits without touching the centered path title; otherwise it SHALL be hidden entirely and never partially drawn. A focused path box SHALL scroll horizontally to keep the caret visible; an unfocused path box whose directory exceeds its width SHALL truncate from the left with a leading `…`. The panel mini-status SHALL drop the same fields in the same order as the Full-mode column ladder (Time, then Date, then Size), truncating the entry name last with `…`; the `N files selected, X bytes` summary SHALL truncate with `…` when it does not fit.

#### Scenario: Clock hides rather than colliding

- **WHEN** the right panel's top border is too narrow for the clock and the centered path title to both fit
- **THEN** the clock is not drawn at all and the path title renders normally

#### Scenario: Command line keeps the caret visible

- **WHEN** the text in a focused path box exceeds the box width
- **THEN** the visible window scrolls so the caret remains on-screen

#### Scenario: Mini-status drops fields in ladder order

- **WHEN** a panel is narrow enough that its mini-status cannot show name, size, date, and time
- **THEN** time is dropped first, then date, then size, and the name truncates with `…` only after all other fields are gone

#### Scenario: Unfocused path box truncates from the left

- **WHEN** a panel's directory is wider than its unfocused path box
- **THEN** the box shows a leading `…` followed by the tail of the path, and nothing paints outside the box
