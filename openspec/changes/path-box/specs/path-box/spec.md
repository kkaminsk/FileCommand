# Spec Delta

## Purpose

Two path boxes under the panels show each panel's current directory, complete folder and file names on Tab, and on Enter put the panel cursor on the entry the text names.

## ADDED Requirements

### Requirement: Two path boxes replace the command line

The system SHALL render, on the row directly above the F-key bar, one path box under each panel, occupying that panel's columns. An unfocused box SHALL show its panel's current directory followed by the platform path separator in the `commandline` theme role, SHALL follow the panel through navigation, tab switches and swaps, and SHALL truncate from the left with a leading `…` when wider than the box. The row SHALL show no shell prompt, and typed text SHALL never be run through a shell.

#### Scenario: Both boxes show their panel's directory
- **WHEN** the left panel is at `C:\NORTON` and the right panel is at `D:\WORK`
- **THEN** the row above the key bar shows `C:\NORTON\` under the left panel and `D:\WORK\` under the right panel

#### Scenario: A box follows its panel
- **WHEN** the left panel navigates into `C:\NORTON\sub`
- **THEN** the left box reads `C:\NORTON\sub\` and the right box is unchanged

#### Scenario: Switching panels changes no box text
- **WHEN** the user presses Tab to make the other panel active
- **THEN** both boxes keep showing their own panel's directory

#### Scenario: A long directory truncates from the left
- **WHEN** a panel's directory is wider than its box
- **THEN** the box shows a leading `…` followed by the tail of the path, and nothing paints outside the box

### Requirement: Focusing a path box

The system SHALL give keyboard focus to the active panel's path box on the focus-path chord (default Ctrl+G, overridable via `key.focus_path`) while the panels own input, and to either box when it is clicked; a click SHALL also make that box's panel active. Focusing SHALL end an active type-ahead search. On focus the box SHALL hold the panel's directory plus separator with the caret at the end, own the terminal cursor, and render in the dialog text-field style.

#### Scenario: Ctrl+G focuses the active panel's box
- **WHEN** the right panel is active at `D:\WORK` and the user presses Ctrl+G
- **THEN** the right box has focus, reads `D:\WORK\` with the caret at the end, and the terminal cursor is in it

#### Scenario: Clicking the other box activates that panel
- **WHEN** the left panel is active and the user clicks the right path box
- **THEN** the right panel becomes active and the right box has focus

#### Scenario: Focusing ends type-ahead
- **WHEN** type-ahead is active with pattern `re` and the user presses Ctrl+G
- **THEN** type-ahead ends, its pattern is gone from the mini-status, and the box has focus

#### Scenario: Rebinding the focus key
- **WHEN** `config.toml` sets `key.focus_path = "ctrl+e"`
- **THEN** Ctrl+E focuses the box and Ctrl+G no longer does

### Requirement: A focused path box owns the keyboard

While a box has focus it SHALL own every key: keys it does not bind SHALL be ignored and SHALL NOT reach the panels, menus or F-key actions. The box SHALL lose focus and reset to the panel's directory when a menu, dialog or overlay opens for any other reason, or when the user clicks outside it; the click is then handled as it would be with no box focused.

#### Scenario: Unbound keys stay inside the box
- **WHEN** a box has focus and the user presses F9, then Alt+F1, then Ctrl+U
- **THEN** no menu or dialog opens, the panels are not swapped, and the box keeps focus and text

#### Scenario: A click outside the box cancels it
- **WHEN** a box has focus with edited text and the user clicks an entry in the left panel
- **THEN** the box closes showing the panel's directory and the clicked entry receives the panel cursor

#### Scenario: A dialog opening for another reason closes the box
- **WHEN** a box has focus and the startup-warning dialog or a file-operation dialog opens
- **THEN** the box closes and resets, and the dialog receives the next key

### Requirement: Panel keys are independent of the path boxes

While no box has focus the panels SHALL keep their full key map regardless of any box text: Tab switches panels, Backspace goes to the parent, Up/Down move the cursor, Left/Right are unbound, `+`/`-`/`*` open group selection and invert selection, and a plain printable key over the panels SHALL be ignored.

#### Scenario: Panel keys keep their meaning without focus
- **WHEN** no box has focus and the user presses Tab, then Backspace, then `+`
- **THEN** the active panel switches, the newly active panel goes to its parent, and the group-select dialog opens; no box text changes

#### Scenario: A printable key over the panels is ignored
- **WHEN** no box has focus and the user types `d`
- **THEN** nothing changes: no box text, no cursor movement, no search

#### Scenario: Up and Down always move the cursor
- **WHEN** no box has focus and the user presses Up
- **THEN** the panel cursor moves up one row

### Requirement: Editing inside a focused path box

A focused path box SHALL be backed by the shared line-edit model (text-field-editing "Caret model and editing keys") and SHALL bind its full key set — printable insert, Backspace, Delete, Left/Right, Home/End, Ctrl+Left/Ctrl+Right, Ctrl+Backspace/Ctrl+Delete — plus Ctrl+V and Shift+Ins paste (text-field-editing "Paste into a text field") and a terminal paste (text-field-editing "Terminal paste arrives as one paste"). The text SHALL scroll horizontally so the caret stays visible.

#### Scenario: Edit mid-path
- **WHEN** the box reads `C:\NORTON\` and the user presses Home, Delete three times and types `D:\`
- **THEN** the box reads `D:\NORTON\` with the caret after the first `\`

#### Scenario: Paste into the box
- **WHEN** the box has focus with the caret at the end of `C:\` and the clipboard holds `Program Files`
- **THEN** after Ctrl+V the box reads `C:\Program Files`

#### Scenario: Long text keeps the caret visible
- **WHEN** the box text is wider than the box and the caret is at the end
- **THEN** the visible window scrolls so the caret remains on-screen

### Requirement: Tab completes the last path segment

While a box has focus, Tab SHALL complete the segment after the last path separator: the text before it names a folder, resolved against the panel directory as in "Enter resolves the path box text", and the text after it is a case-insensitive prefix. Tab SHALL replace that segment with a candidate's on-disk name, and a directory candidate SHALL get a trailing separator.

#### Scenario: Tab completes from the panel listing
- **WHEN** the active panel at `C:\NORTON` lists `docs`, `Downloads` and `data.txt` among others, the box reads `C:\NORTON\d` and the user presses Tab
- **THEN** the box reads `C:\NORTON\docs\` (directories come first) and no completion-read request is emitted

#### Scenario: The on-disk name replaces the typed prefix
- **WHEN** the box reads `C:\NORTON\READ` and the folder holds `readme.md`
- **THEN** Tab makes the box read `C:\NORTON\readme.md`

#### Scenario: Relative and parent-relative folders complete
- **WHEN** the active panel is at `C:\work\proj`, the box reads `..\x` and `C:\work\xray` exists
- **THEN** Tab makes the box read `..\xray\`

### Requirement: Completion candidates and cycling order

The candidates SHALL be every child of the folder whose name starts with the prefix, directories first then files, each in case-insensitive name order. Tab SHALL move to the next candidate and Shift+Tab to the previous one; both SHALL wrap.

#### Scenario: Tab cycles and wraps, Shift+Tab reverses
- **WHEN** the candidates are `docs`, `Downloads` and `data.txt`, the box reads `C:\NORTON\docs\` after a first Tab, and the user presses Tab, Tab, Tab, then Shift+Tab
- **THEN** the text goes `C:\NORTON\Downloads\`, `C:\NORTON\data.txt`, `C:\NORTON\docs\`, then `C:\NORTON\data.txt`

#### Scenario: Matching ignores case
- **WHEN** the folder holds `Readme.md` and `recipes` and the box reads `C:\NORTON\RE`
- **THEN** Tab offers `recipes\` first (a directory) and then `Readme.md`

### Requirement: Completion cycle and mini-status

The prefix in force SHALL be the one present when the cycle started, so repeated Tab presses cycle rather than narrow; any edit other than Tab or Shift+Tab SHALL end the cycle. While a cycle is active the box's panel mini-status SHALL show `k/n matches`; when no child matches it SHALL show `no matches` and Tab SHALL leave the text unchanged.

#### Scenario: Mini-status follows the cycle
- **WHEN** three children match and the user presses Tab three times
- **THEN** the mini-status shows `1/3 matches`, `2/3 matches`, then `3/3 matches`

#### Scenario: An edit ends the cycle
- **WHEN** a cycle is active on `C:\NORTON\docs\` and the user presses Backspace
- **THEN** the mini-status no longer shows a match count, and the next Tab starts a new cycle for the new prefix

#### Scenario: No match
- **WHEN** the box reads `C:\NORTON\zz` and no child of `C:\NORTON` starts with `zz`
- **THEN** Tab leaves the text unchanged and the mini-status shows `no matches`

### Requirement: Completion candidates come from a panel listing when possible

Children SHALL be obtained without filesystem I/O in the reducer: when the folder is the current directory of a panel whose listing is complete, that listing SHALL answer at once and no completion read SHALL be requested.

#### Scenario: The other panel's listing answers
- **WHEN** the left box reads `D:\WORK\re`, the right panel shows `D:\WORK` with a complete listing, and the user presses Tab
- **THEN** the candidates come from the right panel's listing and no completion-read request is emitted

#### Scenario: A streaming listing is not used
- **WHEN** the folder is a panel's current directory but that listing is still streaming
- **THEN** Tab requests a completion read rather than answering from the partial listing

### Requirement: Completion reads run off the event loop

When no complete panel listing covers the folder, the reducer SHALL emit a completion-read request tagged with a request id, run on a worker thread through the `\\?\` long-path abstraction, with the mini-status showing `Reading…` until the reply arrives; Tab meanwhile SHALL be ignored. A reply SHALL be applied only when its id is outstanding and its folder is the one the text names. Names SHALL be reused while the folder part of the text is unchanged.

#### Scenario: A folder outside the panels is read on a worker
- **WHEN** the box reads `D:\Proj\re` and `D:\Proj` is the current directory of neither panel
- **THEN** Tab emits a completion-read request for `D:\Proj` with a request id, the mini-status shows `Reading…`, and the box text is unchanged
- **AND** when the reply lists `README.md`, `release` and `src`, the box reads `D:\Proj\release\` and the mini-status shows `1/2 matches`

#### Scenario: A stale reply is discarded
- **WHEN** a completion read for `D:\Proj` is outstanding and the user edits the box to `E:\x` before it answers
- **THEN** the reply changes nothing and the mini-status does not show matches for `D:\Proj`

#### Scenario: Cached names survive edits within the same folder
- **WHEN** the reply for `D:\Proj` has been applied and the user presses Backspace until the box reads `D:\Proj\s`, then Tab
- **THEN** the box reads `D:\Proj\src\` without a second completion-read request

#### Scenario: Unreadable folder
- **WHEN** the completion read for `\\offline\share` fails
- **THEN** the mini-status shows `no matches`, Tab leaves the text unchanged, and the UI never stopped accepting keys

### Requirement: Enter resolves the path box text

Enter in a focused box SHALL resolve the text, ignoring one trailing separator, to a normalized absolute path by the rules of operation-dialogs "Relative destinations resolve against the source panel directory", against the box's panel directory, and SHALL verify that an entry of that name exists in its parent. When no such entry exists, or the parent cannot be read, the panel's error state SHALL name the path and the box SHALL keep focus and text.

#### Scenario: A relative path resolves against the panel
- **WHEN** the active panel is at `C:\work\proj`, `C:\work\old.txt` exists and the box reads `..\old.txt`
- **THEN** the panel lists `C:\work` with the cursor on `old.txt`

#### Scenario: A drive-relative path resolves like a destination
- **WHEN** the active panel is at `D:\work`, `D:\work\old` exists and the box reads `D:old`
- **THEN** the panel stays at `D:\work` with the cursor on `old`

#### Scenario: A missing path keeps the box open
- **WHEN** the box reads `C:\NORTON\nosuch.txt` and no such entry exists
- **THEN** the panel's directory is unchanged, the panel shows an error naming `C:\NORTON\nosuch.txt`, and the box keeps focus with its text

### Requirement: Enter selects the named entry

When the entry exists, the box's panel SHALL navigate to the entry's parent directory unless it is already there, SHALL place the cursor on the entry (matching the exact name first, else case-insensitively), and the box SHALL close with focus returning to the panel. The panel SHALL NOT enter a directory the text names; a directory is selected in its parent.

#### Scenario: Enter on a listed file lands the cursor
- **WHEN** the active panel is at `C:\NORTON`, lists `readme.md`, the box reads `C:\NORTON\readme.md` and the user presses Enter
- **THEN** the box closes, the panel is still at `C:\NORTON`, and the cursor is on `readme.md`

#### Scenario: Enter on a folder selects it in its parent
- **WHEN** the active panel is at `C:\NORTON`, the box reads `C:\NORTON\docs\` and the user presses Enter
- **THEN** the panel stays at `C:\NORTON` with the cursor on `docs`; it does not enter `docs`

#### Scenario: Enter on a path elsewhere navigates to the parent and lands
- **WHEN** the active panel is at `C:\NORTON`, the box reads `D:\Proj\src\main.rs`, which exists, and the user presses Enter
- **THEN** once the check succeeds the panel lists `D:\Proj\src` with the cursor on `main.rs` and the box is closed

#### Scenario: Case-insensitive landing
- **WHEN** the box reads `C:\NORTON\README.MD` and the listing holds `readme.md`
- **THEN** the cursor lands on `readme.md`

### Requirement: Enter on the panel's own directory or a root

If the resolved path equals the box's panel current directory, Enter SHALL close the box with no other change. If the resolved path is a filesystem root with no parent, the panel SHALL navigate into it and the box SHALL close.

#### Scenario: Enter on the panel's own directory just closes the box
- **WHEN** the box reads `C:\NORTON\` unchanged since focus and the user presses Enter
- **THEN** the box closes, and the panel's directory and cursor are unchanged

#### Scenario: A drive root is entered
- **WHEN** the box reads `D:\` and `D:` exists
- **THEN** the panel navigates to `D:\` and the box closes

#### Scenario: A UNC share root is entered
- **WHEN** the box reads `\\server\share\` and the share exists
- **THEN** the panel navigates to `\\server\share` and the box closes

### Requirement: A listed parent answers the existence check

When the parent is the current directory of a panel whose listing is complete, that listing SHALL answer the existence check, matching the name case-insensitively, with no filesystem I/O in the reducer.

#### Scenario: A listed entry needs no filesystem check
- **WHEN** the active panel at `C:\NORTON` lists `sub` and the box reads `C:\NORTON\sub` when Enter is pressed
- **THEN** the cursor lands on `sub` in response to the Enter itself, without any filesystem check

#### Scenario: The other panel's listing answers the check
- **WHEN** the left box reads `D:\WORK\notes.txt`, the right panel shows `D:\WORK` with `notes.txt` listed, and the user presses Enter
- **THEN** the left panel navigates to `D:\WORK` with the cursor on `notes.txt` and no path-probe request is emitted

### Requirement: The existence check never blocks the event loop

When no complete panel listing covers the parent, the check SHALL run on a worker thread through the `\\?\` long-path abstraction, tagged with a request id, while the box's panel status line shows `Checking <path>…`; the reply SHALL be applied only if it is the most recent check and the box still has focus with the same text, and SHALL otherwise be discarded. The panel's directory SHALL NOT change until the check succeeds.

#### Scenario: An unlisted target is checked off-thread
- **WHEN** the box reads `\\slow\share\x.txt`, which is listed in neither panel, and the user presses Enter
- **THEN** the UI keeps redrawing and accepting keys while the panel's status line shows `Checking \\slow\share\x.txt…`, and the panel's directory is unchanged
- **AND** when the check succeeds the panel lists `\\slow\share` with the cursor on `x.txt`

#### Scenario: A late reply is discarded
- **WHEN** a check for `\\slow\share\x.txt` is outstanding and the user presses Esc before it answers
- **THEN** the reply neither navigates nor sets an error

#### Scenario: Reducer makes no filesystem call
- **WHEN** a unit test applies Enter for `D:\Proj\src\main.rs` with neither panel at `D:\Proj\src`
- **THEN** the returned effects contain a path-probe request for that path with a request id, the panel's `cwd` is unchanged, and the result is the same whether or not the file exists on the test machine

### Requirement: Esc cancels the path box

While a box has focus, Esc SHALL close it, reset its text to the panel's current directory followed by the separator, discard any completion cycle or outstanding check, and return focus to the panel without opening the quit-confirmation dialog.

#### Scenario: Esc reverts the box
- **WHEN** the box reads `C:\NORTON\dra` with a cycle active and the user presses Esc
- **THEN** the box shows `C:\NORTON\`, no dialog opens, and the next Esc requests quit as usual

#### Scenario: Esc discards an outstanding check
- **WHEN** a `Checking …` note is showing for the box's panel and the user presses Esc
- **THEN** the note clears, the box closes, and the check's later reply changes nothing
