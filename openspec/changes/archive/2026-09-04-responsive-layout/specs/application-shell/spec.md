# application-shell Specification (delta)

## RENAMED Requirements

- FROM: `### Requirement: Resize handling with 80x24 minimum and placeholder`
- TO: `### Requirement: Resize handling with 60x16 hard floor and placeholder`

## MODIFIED Requirements

### Requirement: Resize handling with 60x16 hard floor and placeholder

The system SHALL reflow the UI on terminal resize events, laying out the interface at any size at or above the 60x16 hard floor, and MUST draw a `screen.placeholder` "terminal too small" message instead whenever the terminal is below that floor, using a single size check that governs both normal and splash states. The placeholder message SHALL name the floor: "resize to at least 60x16". Between the floor and the 80x24 nominal size the interface renders in the degraded forms defined by the `responsive-layout` capability; at or above 80x24 it renders at full fidelity.

#### Scenario: Reflow at or above the floor

- **WHEN** the terminal is resized to a size at or above 60x16
- **THEN** the UI reflows and lays out its regions to the new dimensions

#### Scenario: Degraded band renders panels, not the placeholder

- **WHEN** the terminal is 70x20 — below the 80x24 nominal size but at or above the floor
- **THEN** the panels, command line, and key bar render in their degraded forms and the placeholder is not shown

#### Scenario: Placeholder below the floor

- **WHEN** the terminal is below 60 columns or below 16 rows
- **THEN** the "terminal too small" placeholder message is drawn instead of the normal layout, naming the 60x16 floor

#### Scenario: Shrinking below the floor during splash

- **WHEN** the terminal shrinks below 60x16 while the startup splash is showing
- **THEN** the placeholder replaces the splash, and the splash does not return when the terminal is enlarged again

#### Scenario: Recovery when resized back up

- **WHEN** the terminal is enlarged from below the floor back to at or above 60x16
- **THEN** the normal layout is drawn again in place of the placeholder
