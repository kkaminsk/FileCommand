## Why

A file-operation job (copy/move/delete/mkdir) only refreshes the panel side(s) that were its formal source/destination at the moment it was launched. The *currently active* directory on each side is already re-checked against the job's affected paths and re-reads correctly today — but a **background tab** (panel-tabs) sitting on the affected directory keeps its own cached entry list and is never told the directory changed. Switching to that tab later still shows the deleted entry (or otherwise-stale contents) until the user forces a fresh read some other way. Tracked as Linear BIG-162: "if a file is deleted in one pane and the other pane is in the same folder it also needs to refresh."

## What Changes

- After any copy/move/delete/mkdir job completes (or is cancelled with partial changes), every tab — active or backgrounded, on either panel side — whose directory matches an affected path is marked stale.
- A background tab marked stale is refreshed automatically the moment it becomes active (Alt+`n` switch, tab close falling back to a neighbor, etc.), rather than waiting on an unrelated manual re-read.
- The currently active tab on each side keeps its existing immediate re-read (no change there); the fix specifically closes the background-tab gap.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `file-operations`: broadens the "Automatic panel re-read on completion" requirement so that every tab (active or background) whose directory matches an affected path is refreshed — active tabs immediately, background tabs on next activation.
- `panel-tabs`: adds the requirement that activating a tab previously marked stale by a completed file-operation triggers a fresh directory read instead of showing its cached (possibly outdated) entries.

## Impact

- `crates/filecommand-core`: `TabData`/`PanelState` gain a per-tab staleness flag; `JobDone` handling walks every tab on both sides (not just the two active cwds) to mark matches; `switch_tab` (and the tab-close fallback) checks the flag on the newly-activated tab and issues a fresh listing when set.
- `crates/filecommand-tui`: no direct changes expected — it already handles `Effect::StartListing` for any panel side; the new effect is simply issued from a different call site (tab switch/close) in addition to job completion.
