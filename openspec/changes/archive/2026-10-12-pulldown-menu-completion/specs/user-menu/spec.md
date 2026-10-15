## ADDED Requirements

### Requirement: Edit the user menu file from the Commands menu

The system SHALL open `usermenu.toml` (at the path the application resolved at startup) when the user activates Commands → Menu file edit, using the F4 editor path — the configured external editor when `editor =` is set, otherwise the built-in editor — and SHALL reload the user menu when the editor closes or returns. A well-formed file SHALL replace the F2 entries immediately; a malformed file SHALL raise the dismissable warning dialog and keep the previous entries, and SHALL NOT overwrite the file.

#### Scenario: Added entry appears in F2
- **WHEN** the user activates Commands → Menu file edit, appends a valid `[[entry]]`, saves, and leaves the editor
- **THEN** pressing F2 lists the new entry

#### Scenario: Malformed edit warns without overwriting
- **WHEN** the user leaves the editor with invalid TOML in the file
- **THEN** a warning dialog names the parse error, F2 still shows the previous entries, and the file on disk is the user's edited text
