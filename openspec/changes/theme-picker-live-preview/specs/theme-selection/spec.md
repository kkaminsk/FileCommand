# theme-selection Specification (delta)

## ADDED Requirements

### Requirement: Live theme preview while the picker is open

While the theme picker is open, the system SHALL render every surface of the frame — panels, key bar, command line, clock, the picker dialog itself, and any overlay drawn above it — by resolving roles through the currently highlighted theme's role table, updating on the next frame after every highlight move. The preview SHALL be render-only: the active theme, the persisted configuration, and the picker's active-theme marker SHALL continue to reflect the applied theme until Enter is pressed. Closing the picker without applying SHALL restore rendering through the applied theme.

#### Scenario: Moving the highlight previews the theme

- **WHEN** the active theme is `nc-classic`, the picker is open, and the user moves the highlight to `purple-lights`
- **THEN** the next frame renders every element by resolving roles in the `purple-lights` table
- **AND** the active theme remains `nc-classic` and `config.toml` is not written

#### Scenario: Esc restores the applied theme's rendering

- **WHEN** the picker is open with `yellow-storm` highlighted and previewed, and the user presses Esc
- **THEN** the dialog closes and the next frame renders entirely through the applied theme's role table

#### Scenario: The marker stays on the applied theme during preview

- **WHEN** the active theme is `nc-classic` and the highlight is on `inverted`
- **THEN** the frame renders in `inverted` while the picker's active-theme marker remains on the `nc-classic` row

#### Scenario: Opening the picker changes nothing visually

- **WHEN** the picker opens with the highlight on the active theme
- **THEN** the rendered frame is identical to the frame before the picker opened, apart from the picker dialog itself
