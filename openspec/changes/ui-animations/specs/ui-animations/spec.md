## ADDED Requirements

### Requirement: Render-only motion model driven by the reducer clock

The system SHALL implement every animation as a pure function of the elapsed time between the reducer's most recent `Tick` clock reading (`State::clock_ms`) and a timestamp the reducer recorded when the animated surface opened or changed. The reducer SHALL transition state instantly exactly as it does without animations and SHALL only additionally record these timestamps from `State::clock_ms`; it SHALL NOT read a clock, delay any transition, or lengthen any hold. Renderers SHALL interpolate from the recorded timestamp, and every key or mouse command SHALL apply to the final (non-animated) state immediately. Animations SHALL use only CP437-heritage glyphs and colors obtained from existing theme roles; no new theme role SHALL be introduced.

#### Scenario: Input during an animation applies to the final state

- **WHEN** a pull-down menu is 30 ms into its 100 ms unfold and the user presses Down
- **THEN** the selection moves to the next enabled item exactly as it would with animations disabled
- **AND** the pull-down continues unfolding around the already-moved highlight

#### Scenario: Animation progress comes from the reducer clock, not a live clock

- **WHEN** a state with an overlay stamped at `clock_ms = 1000` is rendered twice with `clock_ms = 1050`
- **THEN** both renders produce identical output
- **AND** no renderer or reducer reads a system clock to produce the frame

#### Scenario: No new theme roles

- **WHEN** a user theme file that defines exactly today's role set is loaded with animations enabled
- **THEN** it validates and every animation renders using only roles present in that file

### Requirement: Animations are disabled by config or launch flag

The system SHALL animate by default and SHALL render every surface in its final form immediately — identical to the pre-animation rendering — when `animations = false` is set as a top-level key in `config.toml` or the `--noanimations` launch flag is passed. When the flag and config disagree, `--noanimations` SHALL win. The `--noanimations` flag SHALL appear in the `--help` usage block's OPTIONS list. A `State` constructed without configuration (`State::empty`) SHALL have animations disabled so that existing snapshot and reducer tests are unaffected.

#### Scenario: Default is animated

- **WHEN** FileCommand launches with no `animations` key in config and no `--noanimations` flag
- **THEN** the splash, menus, and dialogs animate

#### Scenario: Disabled via config

- **WHEN** `animations = false` is set and the user presses F9
- **THEN** the menu bar and `Left` pull-down render complete on the first frame, byte-identical to the pre-animation rendering

#### Scenario: Flag overrides config

- **WHEN** FileCommand launches with `--noanimations` while config has `animations = true`
- **THEN** no surface animates

#### Scenario: Flag is documented in usage

- **WHEN** the user runs `filecommand --help`
- **THEN** the OPTIONS list includes `--noanimations`

#### Scenario: States built without config do not animate

- **WHEN** a snapshot test renders a state created via `State::empty` with an open dialog
- **THEN** the dialog renders in its final form regardless of `clock_ms`

### Requirement: Pop-up menus unfold and highlights settle

The system SHALL open every pop-up menu — the F9 pull-down boxes, the F2 user menu, and the Enter file-action menu — by unfolding it top-down over 100 ms: the frame's top row appears first, item rows are added downward in order, and the bottom frame edge tracks the last revealed row, so the box is never taller than its final rectangle. A pull-down opened by Left/Right traversal or a hotkey jump SHALL unfold the same way while remaining open and interactive. Whenever the highlighted item changes, the newly highlighted row SHALL render in the `menu.hotkey` role for up to 50 ms and then settle to `menu.highlight`. The menu's final rendering, geometry, and content SHALL be unchanged from today.

#### Scenario: Pull-down unfolds top-down

- **WHEN** the user presses F9 and 50 ms have elapsed on the reducer clock
- **THEN** the `Left` pull-down shows its top frame row and roughly the first half of its item rows, closed by a bottom frame edge
- **AND** at 100 ms or later the pull-down is complete and identical to the pre-animation rendering

#### Scenario: Traversal re-unfolds the adjacent pull-down

- **WHEN** the `Files` pull-down is open and the user presses Right
- **THEN** the `Commands` pull-down begins unfolding immediately with no closed intermediate frame
- **AND** Down pressed during the unfold moves the `Commands` selection

#### Scenario: Highlight settles after moving

- **WHEN** the user presses Down in an open pull-down and the frame is rendered within 50 ms
- **THEN** the newly selected row renders in the `menu.hotkey` style
- **AND** a frame rendered 50 ms or later shows it in `menu.highlight`

#### Scenario: User menu and file-action menu share the treatment

- **WHEN** the user presses F2, or Enter on a file
- **THEN** the user menu or file-action menu unfolds top-down over 100 ms and its highlight settles exactly as the F9 pull-downs do

### Requirement: Modal dialogs open with an iris

The system SHALL open every modal dialog — operation input/confirmation/overwrite/error/progress/summary dialogs, Help and About, theme picker, drive select, find file, fuzzy jump, the quit confirmation, and the startup warning — by revealing the finished dialog through a centered rectangular window that grows from the dialog's center to its full rectangle over 100 ms. The dialog's state, final geometry, content, buttons, and hit regions SHALL be those of the finished dialog from the first frame, so clicks and keys during the iris behave exactly as when it is complete. A dialog that replaces another in the same slot (for example progress → overwrite conflict) SHALL iris again; the quit confirmation layered over another dialog SHALL iris without re-animating the dialog beneath it.

#### Scenario: Dialog grows from its center

- **WHEN** the user presses F7 and 50 ms have elapsed on the reducer clock
- **THEN** a centered window roughly half the dialog's width and height shows the central portion of the finished Make-directory dialog
- **AND** at 100 ms or later the dialog is complete and identical to the pre-animation rendering

#### Scenario: Buttons work during the iris

- **WHEN** the quit-confirmation dialog is 30 ms into its iris and the user presses Enter
- **THEN** the default button activates exactly as it would after the iris completed

#### Scenario: Replacement dialog irises; layered quit dialog does not re-animate the one beneath

- **WHEN** a running copy job raises an overwrite-conflict dialog in place of the progress dialog
- **THEN** the conflict dialog irises in
- **WHEN** the user then presses Esc over the panels with a dialog open and the quit confirmation opens over it
- **THEN** only the quit confirmation irises; the dialog beneath stays fully drawn

### Requirement: Panels wipe in after the splash

The system SHALL, when the splash is replaced by the panels (minimum hold elapsed or dismissing key), reveal the panel screen top-to-bottom over 150 ms, with unrevealed rows drawn in `screen.backdrop`. When the splash was skipped (`splash = false`, `--nosplash`, or below-floor placeholder), the panels SHALL be frame 1 in complete form with no wipe.

#### Scenario: Panels wipe in after the hold

- **WHEN** the splash hold elapses and 75 ms have passed on the reducer clock
- **THEN** roughly the top half of the panel screen is drawn and the rest is backdrop
- **AND** at 150 ms or later the screen is identical to the pre-animation panel rendering

#### Scenario: No wipe when the splash is skipped

- **WHEN** FileCommand launches with `--nosplash`
- **THEN** the first painted frame is the complete panel screen

### Requirement: Loading feedback stays static

The system SHALL NOT animate loading or progress feedback: streaming directory listings, the `Reading… N` mini-status, Info-panel `…` placeholders, drive-select volume-label fill-in, and the progress dialog's counters and byte gauge SHALL continue to render as static text updated in place. Decorative chrome (splash, menus, dialogs) is the only animated surface; this scoping supersedes design doc §4.10's blanket "never uses spinners or animation glyphs" statement for decorative chrome only.

#### Scenario: Streaming listing has no spinner

- **WHEN** a large directory is streaming into a panel with animations enabled
- **THEN** the mini-status shows `Reading… N` updated in place and no spinner, sweep, or glyph cycling appears anywhere in the panel

#### Scenario: Progress gauge is not animated

- **WHEN** a copy job's progress dialog is complete (iris finished) and bytes are transferring
- **THEN** the `█`/`░` gauge and counters change only when a progress event arrives
