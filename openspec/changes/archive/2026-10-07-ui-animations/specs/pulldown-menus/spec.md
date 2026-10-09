## ADDED Requirements

### Requirement: Menu bar entrance sweep

When animations are enabled (`ui-animations`), the system SHALL reveal the F9 menu bar left-to-right over 100 ms: columns of the bar row are painted from the left edge, uncovering the panels' top-border row progressively, with each menu title appearing as the sweep passes its position; the `Left` pull-down unfolds concurrently per `ui-animations`. From 100 ms the bar SHALL be identical to the non-animated bar. Closing the bar SHALL be instant. The bar SHALL be fully interactive from the first frame: hotkey letters and arrows act during the sweep exactly as afterwards.

#### Scenario: Bar sweeps in

- **WHEN** the user presses F9 and 50 ms have elapsed on the reducer clock
- **THEN** roughly the left half of the top row is the black-on-cyan bar showing `Left` and `Files`, the right half still shows the panels' top borders and the clock
- **AND** at 100 ms or later the full-width bar with all five titles is drawn identically to the pre-animation bar

#### Scenario: Hotkey during the sweep

- **WHEN** the bar is 30 ms into its sweep and the user presses the `Commands` hotkey letter
- **THEN** `Commands` becomes the active title and its pull-down opens, as it would after the sweep

#### Scenario: Closing is instant

- **WHEN** the bar is open and the user presses Esc with no pull-down showing
- **THEN** the bar is removed on the next frame with no reverse sweep
