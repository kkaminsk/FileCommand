# application-shell (delta)

## ADDED Requirements

### Requirement: Quit request keys and confirmation

The system SHALL open a quit-confirmation dialog — never exit directly — when the user requests quit via F10, Files → Quit, Esc over the panels, or Ctrl+C from any context except the built-in editor. Esc SHALL request quit whenever the panels own input (no pull-down menu and no modal dialog or overlay open), regardless of command-line content or an active quick filter or type-ahead mode; inside menus, dialogs, the viewer, and editor sub-prompts Esc SHALL retain its existing cancel/dismiss meaning. Ctrl+C SHALL request quit from the panels in any state, the viewer, open pull-down menus, and modal dialogs and overlays; in the built-in editor Ctrl+C SHALL remain Copy. Within the quit-confirmation dialog, Esc SHALL cancel and Ctrl+C SHALL confirm, alongside the existing confirm/cancel keys. Cancelling SHALL restore the prior context exactly — anything open stays open, and command-line text, quick-filter, and type-ahead state survive. Confirming while a file operation is running SHALL abort the job through its existing cancel semantics before the application exits.

#### Scenario: Esc over idle panels asks to quit

- **WHEN** the panels are shown with no menu, dialog, or overlay open and the user presses Esc
- **THEN** the quit-confirmation dialog opens
- **AND** confirming exits the application and cancelling returns to the panels unchanged

#### Scenario: Esc mid-composition asks to quit and cancel preserves the text

- **WHEN** the command-line buffer contains text and the user presses Esc, then cancels the dialog
- **THEN** the quit-confirmation dialog opened instead of the buffer being cleared
- **AND** after cancelling, the buffer still contains the typed text

#### Scenario: Esc under an active quick filter asks to quit

- **WHEN** a quick filter is active and the user presses Esc, then cancels the dialog
- **THEN** the quit-confirmation dialog opened instead of the filter being cleared
- **AND** after cancelling, the filter pattern and narrowed listing are unchanged

#### Scenario: Esc still cancels menus and dialogs

- **WHEN** a pull-down menu or a modal dialog is open and the user presses Esc
- **THEN** the menu or dialog closes as before and no quit-confirmation dialog opens

#### Scenario: Ctrl+C asks to quit from panels, viewer, menus, and dialogs

- **WHEN** the user presses Ctrl+C over the panels, in the viewer, with a pull-down open, or with a modal dialog open
- **THEN** the quit-confirmation dialog opens above the current context
- **AND** cancelling leaves that context exactly as it was (the viewer, menu, or dialog is still open)

#### Scenario: Ctrl+C in the built-in editor still copies

- **WHEN** the built-in editor is open and the user presses Ctrl+C
- **THEN** the editor's Copy action runs and no quit-confirmation dialog opens

#### Scenario: Ctrl+C twice exits, Esc backs out

- **WHEN** the user presses Ctrl+C and then Ctrl+C again in the dialog
- **THEN** the application exits
- **WHEN** the user presses Ctrl+C and then Esc in the dialog
- **THEN** the dialog closes and nothing else changes

#### Scenario: Quit confirmed during a running file operation aborts the job first

- **WHEN** a copy/move/delete job is running and the user presses Ctrl+C, then confirms
- **THEN** the job is aborted through the same path as its cancel action before the application exits
