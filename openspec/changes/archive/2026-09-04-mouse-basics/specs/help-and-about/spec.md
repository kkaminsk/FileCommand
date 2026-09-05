# help-and-about Delta

## MODIFIED Requirements

### Requirement: Help topic list

The system SHALL render, below the identity header, a scrollable list of help topics whose first entry is `About FileCommand`, highlighted (white on black) as the initial cursor position, followed by the v1 topics `Keyboard reference`, `Mouse`, `Panels and display modes`, `File operations`, `Menus`, `Viewer`, `Editor`, `Command line`, `Modern extras`, and `Configuration`, and it SHALL show `↑` / `↓` scroll arrows on the right border only when the list overflows the visible area.

#### Scenario: List opens with About FileCommand highlighted first

- **WHEN** the Help window first appears
- **THEN** the topic list is shown with `About FileCommand` as the first entry and the cursor highlight (white on black) resting on it

#### Scenario: Cursor moves through the topic list

- **WHEN** the user presses Down arrow one or more times within the topic list
- **THEN** the white-on-black highlight moves to the next topic and the list scrolls as needed to keep the highlighted topic visible

#### Scenario: Scroll arrows appear only on overflow

- **WHEN** the topic list is taller than the visible list area
- **THEN** `↑` / `↓` scroll arrows render on the window's right border, and they are absent when the entire list fits

#### Scenario: Mouse topic

- **WHEN** the user opens the `Mouse` topic
- **THEN** its page documents click, double-click, wheel, right-click, Ctrl+click, Shift+drag for native text selection, and the `[mouse]` / `--nomouse` off switches
