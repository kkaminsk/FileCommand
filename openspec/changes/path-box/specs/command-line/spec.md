## REMOVED Requirements

The `command-line` capability is retired by this change. Every requirement below is removed; archiving deletes `openspec/specs/command-line/`. Ctrl+O and the suspended-TUI shell run live on under `application-shell`.

### Requirement: Command-line prompt and printable-key routing
**Reason**: The command line is replaced by two path boxes with dedicated focus; printable keys over the panels no longer have a sink.
**Migration**: `path-box` "Two path boxes replace the command line", "Focusing a path box", "Panel keys are independent of the path boxes".

### Requirement: Run command via shell in suspended-TUI mode
**Reason**: Behaviour unchanged; it only moves home because the capability is retired.
**Migration**: `application-shell` "Shell commands run only from the Run entry and the user menu" and "A suspended shell run restores the terminal".

### Requirement: Command history navigation
**Reason**: No command is ever run, so there is no command history. Up/Down always move the panel cursor.
**Migration**: None. `history.json` keeps its `commands` array for file compatibility; it is preserved on write and never appended to.

### Requirement: Paste filename and path to command line
**Reason**: There is no command line to paste into.
**Migration**: None. Ctrl+Enter and Ctrl+] are unbound; `key.paste_name` and `key.paste_path` are no longer recognised in `config.toml`. Clipboard export of paths and names (`clipboard-export`) is unchanged.

### Requirement: Panels on/off reveals terminal scrollback
**Reason**: Behaviour unchanged; it only moves home because the capability is retired.
**Migration**: `application-shell` "Panels on/off reveals terminal scrollback".

### Requirement: Configurable shell with documented latency tradeoff
**Reason**: Behaviour unchanged; it only moves home because the capability is retired.
**Migration**: `application-shell` "Configurable shell with documented latency tradeoff".

### Requirement: Command-line builtin whitelist
**Reason**: Nothing is typed to run.
**Migration**: None.

### Requirement: cd navigates the active panel or rejects a nonexistent target
**Reason**: Replaced by the path box, which reaches any folder with completion and lands the cursor on the entry.
**Migration**: `path-box` "Enter resolves the path box text", "Enter selects the named entry", "Enter on the panel's own directory or a root", "A listed parent answers the existence check", "The existence check never blocks the event loop".

### Requirement: del and rmdir route into the existing delete-confirmation flow
**Reason**: Typed delete verbs are removed.
**Migration**: F8, the Delete key and the file-action menu's Delete entry open the same delete-confirmation flow.

### Requirement: Clear the command line with a dedicated key
**Reason**: There is no command line to clear; Esc resets a focused path box.
**Migration**: None. Ctrl+Y is unbound; `key.clear_line` is no longer recognised in `config.toml`.

### Requirement: Command history dialog (Alt+F8)
**Reason**: There is no command history.
**Migration**: None. Alt+F8 is unbound and Commands → History is removed (`pulldown-menus` "Menu contents").

### Requirement: cd, del and rmdir targets resolve to a normalized absolute path
**Reason**: The verbs are removed; the resolution rules themselves are owned by `operation-dialogs`.
**Migration**: `path-box` "Enter resolves the path box text" resolves box text by operation-dialogs "Relative destinations resolve against the source panel directory".
