## Why

Users expect `-h`, `-?`, and `--help` to print usage information and exit immediately without launching the TUI — this is a universal CLI convention and the absence is a discoverability gap, especially for new users.

## What Changes

- Recognise `-h`, `-?`, and `--help` as the first argument on the command line
- Print a short usage block to stdout (synopsis, flags, and a pointer to F1 in-app help)
- Exit with code 0 before acquiring the terminal, alternate screen, or mouse capture

## Capabilities

### New Capabilities

_(none)_

### Modified Capabilities

- `application-shell`: new startup requirement — when a help flag is the sole argument, print usage to stdout and exit 0 before any TUI initialisation

## Impact

- `crates/filecommand-tui/src/main.rs` — argument parsing at the top of `main` before `run()`
- No changes to core state, renderer, or installer
