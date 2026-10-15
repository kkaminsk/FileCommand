# Design: command-line-clear-key

## Context

`update.rs:1349` — `Command::CommandLineClear` clears `command_line` and `history_cursor`; nothing outside tests emits it today (`run_command_line` clears the buffer directly after Enter). `Keys` (`config.rs:67-118`) already models overridable chords with `KeyBinding::new(ctrl, alt, shift, key)` and `matches_binding` in `input/mod.rs`. Ctrl+Y is unbound over the panels (the `Char(c)` arm requires `is_plain`). The quit-confirm, file-op DeleteConfirm, and editor quit-confirm arms match `Char('y')` without a modifier guard, so Ctrl+Y currently acts as a plain `y` there, and the destination/rename fields type a literal `y`; those keymaps are not touched by this change.

## Goals / Non-Goals

**Goals:**

- One obvious key to abandon a typed line.
- Keep the user's Esc-quits decision.
- Follow the `key.*` convention.

**Non-Goals:**

- Changing Esc.
- Clearing the quick filter or type-ahead pattern (they have their own exits).
- A clear key inside dialog fields (Home + Ctrl+Delete covers it once `line-editing-in-text-fields` lands).

## Decisions

### D1: Ctrl+Y, fixed meaning, rebindable chord

Ctrl+Y is free on every host terminal crossterm supports and does not collide with Ctrl+U, which `pulldown-menu-completion` assigns to Swap panels (NC's own Ctrl+U). `key.clear_line = "ctrl+y"` is the default.

Alternative rejected: Ctrl+U (Unix line-kill) — it is NC's Swap panels chord and would collide with that proposal.

### D2: No-op on an empty line

The chord only emits `CommandLineClear` when `typing` is true; on an empty line it falls through to nothing, so a mistaken press never does anything surprising.

### D3: Reuse `CommandLineClear` verbatim

No new command. Its existing behavior (clear text + reset `history_cursor`) is exactly "abandon the line, including a half-recalled history entry". With `line-editing-in-text-fields` applied, the same command clears the `LineEdit` and resets the caret.

## Risks / Trade-offs

- [Users of Unix-style Ctrl+U may expect it to clear] → The README notes the rebinding; `key.clear_line = "ctrl+u"` would then shadow Swap panels, documented as the user's choice.

## Open Questions

- None.
