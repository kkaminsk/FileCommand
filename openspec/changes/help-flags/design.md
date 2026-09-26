## Context

`main.rs` in `filecommand-tui` currently calls `run()` unconditionally. There is no argument parsing; unrecognised arguments are silently ignored. The `-h`/`-?`/`--help` flags are absent, so users get no guidance from the CLI.

## Goals / Non-Goals

**Goals:**
- Intercept `-h`, `-?`, `--help` before any terminal acquisition
- Print a concise usage block to stdout and exit 0
- Work correctly in cmd.exe, PowerShell, and Unix shells

**Non-Goals:**
- Full argument-parsing library (`clap`, `argh`) — this change warrants no new dependency
- Localisation of the help text
- Machine-readable output (`--help=json`, completions)
- Forwarding unrecognised flags as errors (out of scope for this change)

## Decisions

**D1 — Manual argv check, no parsing library**

Check `std::env::args().nth(1)` for the three flag strings (`-h`, `-?`, `--help`). The binary has no other flags today, so pulling in a full parser crate would be disproportionate. If flags grow in future the decision can be revisited.

*Alternatives considered:* `clap` — rejected (heavy dependency for a one-flag check); `pico-args` — still a new dep, not justified yet.

**D2 — Print to stdout, not stderr**

`-h` / `--help` is an intentional, successful request for information. POSIX convention and most modern CLIs (`git`, `rg`, `fd`) write help to stdout so the output can be piped or paged. Errors go to stderr; help does not.

**D3 — Exit before terminal acquisition**

The check runs at the very top of `main`, before `run()` is called, so neither the alternate screen nor raw mode is ever entered. This keeps the terminal in a normal state for shell scripting.

**D4 — Usage text content**

One-liner synopsis, flag table, and a pointer to F1 in-app help. Version string is embedded via `env!("CARGO_PKG_VERSION")` — the same source of truth used by the splash and Info panel.

## Risks / Trade-offs

- `-?` is unusual on Unix but is the Windows help-flag convention (cmd.exe, PowerShell `Get-Help`). Including it costs nothing. → No mitigation needed.
- Hard-coded help text can drift from actual behaviour. → Kept minimal (flags only, not full key reference) to reduce maintenance surface.

## Migration Plan

No migration required — additive change with no effect on existing invocations.
