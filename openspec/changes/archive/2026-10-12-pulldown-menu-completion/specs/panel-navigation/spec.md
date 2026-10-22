## ADDED Requirements

### Requirement: Swap panels (Ctrl+U)

The system SHALL exchange the left and right panels' complete state — directory, tabs, listing, cursor, scroll offset, selection, sort mode, filter, display mode, and git info — on Ctrl+U or Commands → Swap panels, keeping the active side and the split percentage unchanged, without re-reading either directory. A swap requested while either panel's listing is still streaming SHALL be ignored. After a swap the system SHALL re-issue each panel's pending Info-mode and git-info queries so no reply from before the swap is applied to the wrong panel.

#### Scenario: Swap keeps focus on the same side
- **WHEN** the left panel is active showing `C:\a` and the right shows `D:\b`, and the user presses Ctrl+U
- **THEN** the left panel shows `D:\b`, the right shows `C:\a`, the left panel is still active, and the command-line prompt reads `D:\b>`

#### Scenario: Swap carries tabs and selection
- **WHEN** the right panel has three tabs and two selected entries and the user presses Ctrl+U
- **THEN** the left panel now has those three tabs and the same two selected entries

#### Scenario: Swap refused while reading
- **WHEN** the right panel's listing is still streaming and the user presses Ctrl+U
- **THEN** nothing changes

#### Scenario: Git info follows the swap
- **WHEN** the left panel is inside a git repository and the user presses Ctrl+U
- **THEN** the right panel's border shows that repository's branch suffix after its re-issued query resolves, and the left panel shows none
