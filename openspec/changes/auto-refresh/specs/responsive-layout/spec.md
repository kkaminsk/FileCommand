## MODIFIED Requirements

### Requirement: F-key bar degradation forms

The system SHALL render the F-key bar in the widest of three canonical forms that fits the terminal width, never truncating a slot mid-label: the full form `1Help 2Menu 3View 4Edit 5Copy 6RenMov 7Mkdir 8Delete 9PullDn 10Quit 12Refresh` (77 columns), the short form `1Hlp 2Mnu 3Vew 4Edt 5Cpy 6Ren 7Mkd 8Del 9Pdn 10Qit 12Rfr` (56 columns), and the numbers-only form `1 2 3 4 5 6 7 8 9 10 12` (23 columns). All eleven slots SHALL always be present in whichever form renders; no slot numbered 11 is drawn. The same widest-form-that-fits rule SHALL govern the Ctrl and Alt modifier label variants and the viewer and editor F-key bars, each with its own short-form labels of the same shape; those bars gain no twelfth slot.

#### Scenario: Full form at nominal width

- **WHEN** the terminal is 80 columns wide
- **THEN** the key bar renders the full form with all eleven labeled slots, ending in `12Refresh`

#### Scenario: Short form at the floor

- **WHEN** the terminal is 60 columns wide
- **THEN** the key bar renders the short form `1Hlp 2Mnu 3Vew 4Edt 5Cpy 6Ren 7Mkd 8Del 9Pdn 10Qit 12Rfr` with all eleven slots visible

#### Scenario: No mid-label truncation

- **WHEN** the terminal width falls between two forms' required widths
- **THEN** the narrower form renders in full; no slot is ever partially drawn
