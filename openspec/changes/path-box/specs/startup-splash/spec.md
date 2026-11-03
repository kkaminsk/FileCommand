## MODIFIED Requirements

### Requirement: Minimum hold and key dismissal

The system SHALL hold the splash for a minimum of 800 ms measured via the injected `Clock`, after which it is replaced by the panels even if the initial listing is still streaming. Any key press SHALL dismiss the splash immediately without waiting for the minimum hold, and the dismissing key event SHALL be consumed — never forwarded to the path boxes or panels.

#### Scenario: Minimum hold elapses with no input

- **WHEN** no key is pressed and 800 ms have elapsed on the injected `Clock` since the splash first painted
- **THEN** the splash is replaced by the panels
- **AND** the panels render even if the initial directory listing is still streaming

#### Scenario: Key press dismisses before the minimum hold

- **WHEN** a key is pressed 200 ms after the splash first painted
- **THEN** the splash is dismissed immediately, before the 800 ms minimum hold elapses

#### Scenario: Dismissing key is consumed

- **WHEN** a key press dismisses the splash
- **THEN** that key event is consumed and is not forwarded to the path boxes or the panels
