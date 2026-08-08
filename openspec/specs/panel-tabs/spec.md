# panel-tabs Specification

## Purpose
TBD - created by archiving change m5-editor-and-modern-extras. Update Purpose after archive.
## Requirements
### Requirement: Per-panel tab list with independent state

Each panel SHALL own a list of tabs and an active-tab index, where every tab holds a fully independent panel state — current directory, cursor position, selection set, sort mode, filter, and display mode. Switching, opening, or closing tabs SHALL only affect the active panel; the opposite panel's tab list is unaffected. On startup each panel SHALL have exactly one tab.

#### Scenario: Each tab retains its own directory and state

- **WHEN** the active panel has two tabs, tab 1 at `C:\A` sorted by Size with three entries selected, and tab 2 at `C:\B` sorted by Name with no selection
- **THEN** switching from tab 1 to tab 2 shows `C:\B` sorted by Name with no selection, and switching back to tab 1 restores `C:\A` sorted by Size with the same three entries still selected and the same cursor position

#### Scenario: Tab operations are scoped to the active panel

- **WHEN** the left panel is active with one tab and the right panel has three tabs
- **THEN** opening or closing a tab changes only the left panel's tab list, leaving the right panel's three tabs and its active-tab index unchanged

### Requirement: New tab (Ctrl+T)

The active panel SHALL open a new tab in response to Ctrl+T. The new tab SHALL be inserted into the active panel's tab list, its initial directory and state SHALL be inherited from the tab that was active when Ctrl+T was pressed, and the new tab SHALL become the active tab.

#### Scenario: Ctrl+T opens and activates a new tab

- **WHEN** the active panel has one tab showing `C:\Work` and the user presses Ctrl+T
- **THEN** the panel has two tabs, the newly created tab is active and shows `C:\Work`, and the tab strip becomes visible

#### Scenario: New tab does not disturb the originating tab

- **WHEN** the user presses Ctrl+T while a tab has a cursor and selection set
- **THEN** the original tab retains its cursor and selection, and navigating in the new tab leaves the original tab's directory and state unchanged

### Requirement: Close tab (Ctrl+W)

The active panel SHALL close the active tab in response to Ctrl+W, removing it from the tab list and activating an adjacent tab. When only one tab remains, Ctrl+W SHALL be a no-op — the panel SHALL always retain at least one tab.

#### Scenario: Ctrl+W closes the active tab and activates a neighbor

- **WHEN** the active panel has three tabs with tab 2 active and the user presses Ctrl+W
- **THEN** tab 2 is removed, the panel has two tabs, and an adjacent tab becomes active

#### Scenario: Ctrl+W is a no-op with a single tab

- **WHEN** the active panel has exactly one tab and the user presses Ctrl+W
- **THEN** the tab is not closed, the panel still has one tab, and the tab strip remains hidden

### Requirement: Switch tab (Alt+1..9)

The active panel SHALL activate the tab at the one-based position indicated by Alt+1 through Alt+9. When no tab exists at the requested position the key SHALL be a no-op, leaving the active tab unchanged.

#### Scenario: Alt+n activates the nth tab

- **WHEN** the active panel has four tabs and the user presses Alt+3
- **THEN** the third tab becomes active and its directory and state are displayed

#### Scenario: Alt+n out of range is ignored

- **WHEN** the active panel has two tabs and the user presses Alt+5
- **THEN** the active tab is unchanged and no tab is opened

### Requirement: Tab strip visibility

A single compact tab-strip row SHALL be rendered above the active panel's body only when that panel has two or more tabs; with exactly one tab the strip SHALL be hidden and the panel SHALL keep its full height. When the strip is shown the panel body SHALL shrink by exactly one row to make room for it.

#### Scenario: Strip hidden with one tab

- **WHEN** a panel has exactly one tab
- **THEN** no tab strip is drawn and the panel occupies its full height

#### Scenario: Strip appears and reclaims a row with two tabs

- **WHEN** a panel transitions from one tab to two tabs
- **THEN** a single tab-strip row appears above the panel body and the body shrinks by one row

### Requirement: Tab label rendering and active styling

Each tab SHALL render as ` n:NAME ` where `n` is the one-based tab number and `NAME` is the tab directory's basename uppercased. The active tab SHALL use the `tab.active` role (black on cyan) and inactive tabs the `tab.inactive` role (cyan on blue), on a blue strip background. Rendering SHALL use only ANSI-16 named colors and the single-cell CP437-heritage glyph set (no icons or emoji), per the rendering policy.

#### Scenario: Active and inactive tabs are styled distinctly

- **WHEN** the strip shows tab 1 (active) and tab 2 (inactive)
- **THEN** tab 1 renders as ` 1:NAME ` in the `tab.active` role and tab 2 renders as ` 2:NAME ` in the `tab.inactive` role

#### Scenario: Basenames are uppercased

- **WHEN** a tab's directory is `C:\projects\filecommand`
- **THEN** its label reads ` n:FILECOMMAND `

### Requirement: Stepwise label shrinking and scrolling overflow

When the tabs do not all fit on the strip width, labels SHALL shrink stepwise — first from ` n:NAME ` to a truncated ` n:NAM… `, then to ` n ` — before any scrolling occurs. If the tabs still overflow at the minimum label form, the strip SHALL scroll so the active tab remains visible, drawing `◄` and/or `►` overflow markers (`tab.inactive` role, cyan on blue) at the strip ends where hidden tabs exist. Shrinking and scrolling SHALL be a deterministic function of the tab set, active index, and strip width.

#### Scenario: Labels truncate before scrolling

- **WHEN** full ` n:NAME ` labels do not fit but truncated ` n:NAM… ` labels do
- **THEN** the strip renders truncated labels with the `…` glyph and does not scroll

#### Scenario: Labels collapse to number-only when very tight

- **WHEN** even truncated labels do not fit but ` n ` number-only labels do
- **THEN** each tab renders as ` n ` with no basename

#### Scenario: Strip scrolls to keep the active tab visible

- **WHEN** the tabs overflow the strip even at minimum ` n ` form and the active tab is beyond the visible window
- **THEN** the strip scrolls so the active tab is visible and an `◄` or `►` overflow marker is drawn at each end that has hidden tabs

