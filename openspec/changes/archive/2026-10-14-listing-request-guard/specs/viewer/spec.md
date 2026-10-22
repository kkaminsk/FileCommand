## ADDED Requirements

### Requirement: Search stops when superseded or when the viewer closes

The system SHALL stop an F7 search that is still scanning when the viewer closes or when the user starts another F7 search in the same viewer. The search SHALL check for this before reading each chunk, SHALL stop at the next chunk boundary once signalled, SHALL release its own handle on the file, and SHALL deliver no result. A superseded search's result SHALL never move the view or set the match highlight.

#### Scenario: Closing the viewer stops a running search

- **WHEN** an F7 search for a pattern that does not occur is scanning a multi-GB file and the user presses F10
- **THEN** the viewer closes, the search stops within one chunk read, and the search no longer holds the file open

#### Scenario: A newer search supersedes a running one

- **WHEN** F7 search A is still scanning and the user starts F7 search B in the same viewer
- **THEN** search A stops at its next chunk boundary and only search B's result moves the view and highlights a match
