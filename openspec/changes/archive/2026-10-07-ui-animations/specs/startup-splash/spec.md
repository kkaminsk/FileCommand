## MODIFIED Requirements

### Requirement: Frame-1 splash rendering

The system SHALL render the startup splash as the very first painted frame from static identity data — a solid blue backdrop with a horizontally and vertically centered double-line box containing the product name, version, copyright, and tribute lines — so that first paint never waits on any I/O. When animations are enabled (`ui-animations`), frame 1 SHALL be the backdrop plus the first frame of the splash entrance animation and the complete box SHALL be reached within the minimum hold; when animations are disabled the complete box SHALL be frame 1. The identity lines (name, version, copyright, tribute) SHALL be defined in a single place in `core` and used verbatim; the version string SHALL be the crate version. The terminal cursor SHALL be hidden while the splash is shown, and all colors SHALL come from the `splash.*` theme roles.

#### Scenario: Splash is the first frame

- **WHEN** FileCommand launches at or above 80×24 with the splash enabled
- **THEN** the first painted frame is the splash: a solid blue backdrop with, when animations are disabled, the complete centered double-line box, or, when enabled, the opening frame of its entrance
- **AND** the completed box contains the product name, `Version <crate-version>`, the copyright line, and the tribute line
- **AND** it paints before the first directory listing has completed

#### Scenario: Splash uses theme roles and hides the cursor

- **WHEN** the splash renders under `nc-classic`
- **THEN** the frame is drawn in `splash.frame` (cyan on blue), the name in `splash.title` (bright-white), the version in `splash.version` (white), and the copyright/tribute in `splash.text` (cyan)
- **AND** the terminal cursor is hidden

#### Scenario: Identity lines are a single source of truth

- **WHEN** the splash renders its identity lines
- **THEN** they are read from the single `core` definition of the identity lines (name, version, copyright, tribute) rather than a splash-local copy

#### Scenario: Mono theme rendering

- **WHEN** the splash renders under `nc-mono`
- **THEN** it renders white on black

## ADDED Requirements

### Requirement: Splash entrance animation

When animations are enabled, the system SHALL animate the splash entrance within the 800 ms minimum hold using only `splash.*` roles and the existing box glyphs: from 0 to 200 ms the double-line frame SHALL trace itself clockwise from the top-left corner; from 200 ms each identity line SHALL reveal outward from its center over 120 ms, starting 80 ms after the previous line, with newly revealed characters drawn in `splash.frame` for up to 66 ms before settling to the line's final role; from 560 to 760 ms a 3-cell window in `splash.version` SHALL sweep once left-to-right across the product name. From 760 ms the splash SHALL be identical to the non-animated splash. A key press at any point SHALL dismiss the splash entirely (not skip to its final frame) and be consumed, exactly as today; the minimum hold SHALL remain 800 ms.

#### Scenario: Frame traces in

- **WHEN** 100 ms have elapsed on the reducer clock since the splash started
- **THEN** roughly half the box perimeter is drawn, beginning at the top-left corner and proceeding clockwise, and no identity text is visible yet

#### Scenario: Identity lines reveal outward

- **WHEN** 400 ms have elapsed
- **THEN** the frame is complete, the product name and version lines are fully revealed, and the copyright line is partially revealed from its center outward

#### Scenario: Title sweep then steady state

- **WHEN** 650 ms have elapsed
- **THEN** a 3-cell `splash.version` window is positioned part-way across the product name
- **AND** at 760 ms or later the splash is identical to the pre-animation splash

#### Scenario: Key mid-animation dismisses entirely

- **WHEN** a key is pressed 150 ms after the splash started
- **THEN** the splash is dismissed and the panels appear (wiping in per `ui-animations`), the key is consumed, and no further splash frame is drawn

#### Scenario: Disabled renders the finished splash

- **WHEN** animations are disabled and the splash is shown
- **THEN** every splash frame is the complete box
