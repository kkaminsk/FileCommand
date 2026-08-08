# info-panel Specification

## Purpose
TBD - created by archiving change m3-command-line-and-menus. Update Purpose after archive.
## Requirements
### Requirement: Info display mode replaces the panel body

The system SHALL provide an Info display mode that, when active for a panel, replaces that panel's normal listing body with a vertical stack of single-line-framed boxes drawn inside the panel's double-line border, and SHALL toggle Info mode on and off for the active panel via the Ctrl+L binding without affecting the opposite panel.

#### Scenario: Toggling Info mode on the active panel

- **WHEN** a panel is focused in a normal display mode and the user presses Ctrl+L
- **THEN** that panel's body is replaced by the stacked Info boxes and the opposite panel's display mode is unchanged

#### Scenario: Toggling Info mode back off

- **WHEN** the active panel is in Info mode and the user presses Ctrl+L again
- **THEN** the panel returns to its previous display mode showing its directory listing

#### Scenario: Boxes are single-line framed and stacked

- **WHEN** a panel is in Info mode
- **THEN** its content renders as vertically stacked boxes each drawn with single-line CP437 frame glyphs (`─ │ ┌ ┐ └ ┘`) within the panel's double-line border

### Requirement: Info mode content set

The Info display SHALL present the version banner, a memory figure, the current drive's total and free byte counts, the drive's volume label, the drive's serial number, and the file and directory counts of the panel's current directory, each shown as a labelled field.

#### Scenario: All info fields are present

- **WHEN** a panel is in Info mode
- **THEN** the display includes the version banner, memory, drive total bytes, drive free bytes, volume label, serial number, file count, and directory count

#### Scenario: Field labels and values are visually distinct

- **WHEN** the Info display renders a labelled field
- **THEN** the label text uses the `info.label` role (cyan) and the value text uses the `info.value` role (bright-yellow)

### Requirement: Version banner is the shared identity source

The version banner in the Info display SHALL render the identity lines (product name, version, copyright, tribute) verbatim from the single shared identity source used by the startup splash and the About dialog, styled with the `info.banner` role (bright-white).

#### Scenario: Banner matches the shared identity lines

- **WHEN** a panel is in Info mode
- **THEN** the version banner shows the same product name, version, copyright, and tribute strings as the startup splash and About dialog, rendered in bright-white

### Requirement: Async values fill in place without blocking

Info values that require a drive or directory query — drive total bytes, drive free bytes, volume label, serial number, file count, and directory count — SHALL render as `…` immediately and SHALL be replaced in place when their background query resolves, and neither computing nor awaiting these values SHALL block a paint or an input event.

#### Scenario: Placeholder shown before resolution

- **WHEN** Info mode is first displayed and the drive/directory queries have not yet completed
- **THEN** each async value renders as `…` while the panel remains fully interactive to input

#### Scenario: Value replaces its placeholder when resolved

- **WHEN** a background query for an async Info value completes
- **THEN** that field's `…` is replaced in place by the resolved value without redrawing or disturbing the other fields

#### Scenario: Slow or absent query never freezes the UI

- **WHEN** a drive query for total/free space, volume label, or serial is slow to resolve (e.g. a network drive)
- **THEN** the Info display continues to paint and accept input with the unresolved fields still showing `…`

### Requirement: Stale async results are discarded

When an Info background query resolves, the system SHALL apply the result only if the panel is still in Info mode and still targeting the same drive and directory the query was issued for, and SHALL discard results whose target drive or directory no longer matches.

#### Scenario: Result for a changed drive is dropped

- **WHEN** an async Info value resolves but the panel has since switched to a different drive or directory
- **THEN** the resolved value is discarded and the currently displayed fields are not altered by it

### Requirement: Info mode rendering uses static text only

The Info display SHALL use only ANSI-16 named colors via its assigned theme roles (`info.label`, `info.value`, `info.banner`) and only CP437-heritage glyphs, and SHALL NOT use spinners, animation, or any non-static loading indicator for pending values.

#### Scenario: No animation for pending values

- **WHEN** one or more Info values are still pending
- **THEN** their pending state is shown only as the static `…` glyph, with no spinner or animated indicator anywhere in the display

