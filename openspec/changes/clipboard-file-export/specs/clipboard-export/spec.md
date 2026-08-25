# clipboard-export Delta

## ADDED Requirements

### Requirement: Clipboard payloads and scope

The system SHALL provide three clipboard actions — Files, Paths, Names — each acting on the same scope as Copy (F5): the active panel's selection set when non-empty, otherwise the cursor entry; the parent-directory pseudo-entry SHALL never be included. Files SHALL place the entries on the OS clipboard as file objects; Paths SHALL place one absolute path per line as plain text; Names SHALL place one file name per line as plain text. No file-system mutation SHALL occur.

#### Scenario: Files uses the selection set

- **WHEN** three entries are selected in `C:\NORTON` and the user invokes Files
- **THEN** the clipboard holds file objects for exactly those three entries and the selection is unchanged

#### Scenario: Cursor entry when nothing is selected

- **WHEN** no entries are selected, the cursor is on `README.md`, and the user invokes Paths
- **THEN** the clipboard text is `C:\NORTON\README.md`

#### Scenario: Parent entry is never copied

- **WHEN** the cursor is on `..` with no selection and the user invokes Files
- **THEN** nothing is written to the clipboard and the mini-status reports that there is nothing to copy

### Requirement: Clipboard key bindings

The system SHALL bind Ctrl+C to Files over the panels (rebindable via `key.clipboard_files` in `config.toml`), SHALL accept Ctrl+Ins as a fixed alias for Files, and SHALL bind Ctrl+Shift+Ins to Paths (rebindable via `key.clipboard_paths`); these Insert chords SHALL take precedence over the plain-Insert selection toggle. Names SHALL be reachable through the Files pull-down only. Ctrl+C SHALL NOT request quit in any context; inside the built-in editor Ctrl+C SHALL remain the editor's Copy.

#### Scenario: Ctrl+C copies files over the panels

- **WHEN** the panels own input and the user presses Ctrl+C
- **THEN** the Files action runs and no quit-confirmation dialog opens

#### Scenario: Ctrl+Ins alias

- **WHEN** the user presses Ctrl+Ins over the panels
- **THEN** the Files action runs exactly as for Ctrl+C

#### Scenario: Rebinding the files chord

- **WHEN** `config.toml` sets `key.clipboard_files = "ctrl+k"`
- **THEN** Ctrl+K runs Files and Ctrl+C does nothing over the panels, while Ctrl+Ins still runs Files

#### Scenario: Editor keeps Ctrl+C as Copy

- **WHEN** the built-in editor is open and the user presses Ctrl+C
- **THEN** the editor's text Copy runs and the OS clipboard file action does not

### Requirement: Windows file-object payload

On Windows the Files action SHALL write `CF_HDROP` (a `DROPFILES` header followed by NUL-terminated UTF-16 absolute paths and a double-NUL terminator) together with a `Preferred DropEffect` value of `DROPEFFECT_COPY`. Every path SHALL be absolute, SHALL NOT carry the `\\?\` prefix, and `\\?\UNC\server\share\...` SHALL be rewritten as `\\server\share\...`.

#### Scenario: Paste in Explorer copies the files

- **WHEN** the user invokes Files on `report.docx` and pastes in Explorer
- **THEN** Explorer copies `report.docx` into its folder and the source is untouched

#### Scenario: Long-path prefix is stripped

- **WHEN** the active panel's directory is held internally as `\\?\C:\very\long\path`
- **THEN** the `CF_HDROP` entry reads `C:\very\long\path\<name>`

#### Scenario: UNC prefix is rewritten

- **WHEN** the directory is held internally as `\\?\UNC\srv\share\dir`
- **THEN** the `CF_HDROP` entry reads `\\srv\share\dir\<name>`

### Requirement: Clipboard busy retry

When the OS clipboard cannot be opened because another process holds it, the system SHALL retry a bounded number of times with a short back-off and, on final failure, SHALL report `Clipboard busy — try again` without blocking the UI thread for more than a fraction of a second.

#### Scenario: Transient lock succeeds on retry

- **WHEN** the clipboard is locked for the first attempt and free on the second
- **THEN** the payload is written and success feedback is shown

#### Scenario: Persistent lock reports failure

- **WHEN** every attempt fails to open the clipboard
- **THEN** the mini-status shows `Clipboard busy — try again` in the error role and nothing else changes

### Requirement: Non-Windows fallback

On non-Windows platforms Paths and Names SHALL write plain text; Files SHALL fall back to the Paths text payload and the feedback SHALL state that file objects are unsupported on this platform.

#### Scenario: Files falls back to paths text

- **WHEN** the user invokes Files on Linux
- **THEN** the clipboard holds the absolute paths as text and the mini-status reads `Paths copied (file objects unsupported here)`

### Requirement: Clipboard feedback

The system SHALL show the outcome of a clipboard action in the active panel's mini-status line — `N files copied to clipboard`, `Path copied: <path>`, `N paths copied`, `N names copied` — and SHALL restore the normal mini-status content on the next key press or after approximately three seconds, whichever comes first.

#### Scenario: Success feedback expires

- **WHEN** the user invokes Files on three entries and presses no key for three seconds
- **THEN** the mini-status shows `3 files copied to clipboard` and then reverts to `3 files selected, X bytes`

#### Scenario: Next key clears feedback

- **WHEN** feedback is showing and the user presses Down
- **THEN** the cursor moves and the mini-status shows the normal display

### Requirement: Clipboard actions in menus

The Files pull-down SHALL list, as a separated group after Delete, `Copy to clipboard` (shortcut hint `Ctrl-C`), `Copy path(s)` (shortcut hint `Ctrl-Sh-Ins`), and `Copy name(s)`; the file-action menu SHALL list `Send to clipboard` as its last entry, running Files for the menu's target scope. Menu activation SHALL behave exactly as the corresponding key.

#### Scenario: Files menu group

- **WHEN** the user opens the Files pull-down
- **THEN** the three clipboard items appear as a group between Delete and Attributes with their shortcut hints right-aligned

#### Scenario: Send to clipboard from the action menu

- **WHEN** the action menu is open for `notes.txt` and the user activates `Send to clipboard`
- **THEN** the menu closes and the clipboard holds a file object for `notes.txt`
