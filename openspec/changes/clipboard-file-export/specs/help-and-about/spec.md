# help-and-about Delta

## MODIFIED Requirements

### Requirement: Help topic pages

The system SHALL replace the topic list with the selected topic's page inside the same Help window, rendering static text compiled into the binary, and Esc SHALL return from a topic page to the topic list rather than closing the window; the `Keyboard reference` page SHALL document the Ctrl/Alt F-key-bar variants; the `Modern extras` page SHALL document the clipboard actions and their bindings (Ctrl+C / Ctrl+Ins for files, Ctrl+Shift+Ins for paths, menu-only names).

#### Scenario: Selecting a topic replaces the list with its page

- **WHEN** the user opens the `Menus` topic from the list
- **THEN** the topic list is replaced within the same window by the `Menus` page rendered from compiled-in static text, with no filesystem read performed

#### Scenario: Esc returns from a topic page to the list

- **WHEN** the user presses Esc while viewing a topic page
- **THEN** the window returns to the topic list with the previously highlighted topic still selected, and the window remains open

#### Scenario: Keyboard reference documents the modifier bar variants

- **WHEN** the user opens the `Keyboard reference` topic
- **THEN** its page includes documentation of the Ctrl and Alt F-key-bar label variants

#### Scenario: Modern extras documents the clipboard bindings

- **WHEN** the user opens the `Modern extras` topic
- **THEN** its page lists the Files, Paths, and Names clipboard actions with their key bindings
