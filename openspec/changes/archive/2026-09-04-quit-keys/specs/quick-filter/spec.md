# quick-filter (delta)

## MODIFIED Requirements

### Requirement: Clearing the quick filter

The system SHALL exit quick-filter mode and clear the filter when the user presses the quick-filter activation key (Ctrl+P by default, honoring any remap) while a filter is active — the activation key toggles the filter — restoring the full (unfiltered) entry list and the normal mini-status display. Esc SHALL NOT exit the filter; over the panels it requests application quit, and cancelling that dialog leaves the filter active (application-shell "Quit request keys and confirmation"). The system SHALL preserve the current sort mode and selection set across activation and clearing of the filter; the quick filter narrows what is shown but does not alter the underlying listing.

#### Scenario: Activation key toggles the filter off

- **WHEN** a quick filter is active and the user presses the quick-filter activation key
- **THEN** quick-filter mode exits and the pattern is discarded
- **AND** the panel shows the full unfiltered entry list
- **AND** the mini-status line reverts to its normal current-entry / selection display

#### Scenario: Esc leaves the filter in place

- **WHEN** a quick filter is active and the user presses Esc, then cancels the quit-confirmation dialog
- **THEN** the filter pattern and the narrowed listing are unchanged

#### Scenario: Selection survives filtering

- **WHEN** entries are selected, the user applies then clears a quick filter
- **THEN** the previously selected entries remain selected after the filter is cleared
