# startup-splash Specification (delta)

## MODIFIED Requirements

### Requirement: Resize and below-minimum-size behavior

The system SHALL position the splash box on every terminal resize via the unified overlay geometry rule (`responsive-layout`), keeping it centered; the 48×10 box fits at every supported size down to the 60×16 floor, so the splash renders throughout the degraded band. If the terminal is below the 60×16 floor at startup, the splash SHALL be skipped in favor of the "terminal too small" placeholder. If the terminal shrinks below the floor while the splash is shown, the splash SHALL be replaced by the placeholder and SHALL NOT return when the terminal grows back.

#### Scenario: Box re-centers on resize

- **WHEN** the terminal is resized while the splash is shown and remains at or above 60×16
- **THEN** the splash box is re-centered horizontally and vertically in the new dimensions

#### Scenario: Splash renders in the degraded band

- **WHEN** FileCommand launches at terminal size 60×16 with the splash enabled
- **THEN** the splash renders as the first frame, its 48×10 box centered

#### Scenario: Below the floor at startup

- **WHEN** FileCommand launches with the terminal below 60×16
- **THEN** the splash is skipped and the "terminal too small" placeholder is drawn instead

#### Scenario: Shrinking below the floor mid-splash

- **WHEN** the terminal shrinks below 60×16 while the splash is shown
- **THEN** the splash is replaced by the "terminal too small" placeholder
- **AND** the splash does not return when the terminal is enlarged back to at or above 60×16
