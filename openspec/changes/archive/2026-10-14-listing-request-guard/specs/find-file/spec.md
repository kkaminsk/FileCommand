## ADDED Requirements

### Requirement: Dismissed or superseded searches stop

When a find-file search is no longer wanted — the dialog is dismissed with Esc, a result is confirmed while the walk is still running, or a new search replaces it — the system SHALL signal the subtree walk to stop. The walk SHALL check that signal before reading each directory and between the entries it examines, SHALL stop promptly once it is set, and SHALL send no further results. Results of a stopped or superseded walk SHALL never appear in any find-file dialog.

#### Scenario: Esc stops the walk

- **WHEN** a search started at `C:\` with the pattern `a` is still walking the subtree and the user presses Esc
- **THEN** the dialog closes and the walk stops before reading another directory, producing no further results

#### Scenario: Confirming a result while the walk is running stops it

- **WHEN** the user highlights a result and presses Enter while the walk is still running
- **THEN** the panel navigates to the result's directory and the walk stops

#### Scenario: A new search after Esc runs alone

- **WHEN** the user presses Esc during a running search, reopens the dialog with Alt+F7, and submits a different pattern
- **THEN** only the new search's walk is running and only its matches appear in the result list
