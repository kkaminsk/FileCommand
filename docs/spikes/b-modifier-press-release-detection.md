# Spike (b): standalone modifier press/release detection

**Question:** can FileCommand detect a bare Shift/Ctrl/Alt press-and-release
(with no other key involved), to support live F-key bar relabeling — the
classic Norton Commander behavior where the bottom bar's labels change while
Shift or Ctrl is held (e.g. `5Copy` becomes `5RenMv`-style alternates)? This
gates whether that feature is feasible at all before any later milestone
scopes it.

## Method

Probe crossterm's event stream for standalone modifier key events under
default raw-mode settings, and separately under the Kitty keyboard protocol
enhancement flags crossterm exposes
(`PushKeyboardEnhancementFlags`/`KeyboardEnhancementFlags::REPORT_EVENT_TYPES`
+ `DISAMBIGUATE_ESCAPE_CODES`).

## Findings

- **Default Windows console mode (no enhancement flags):** the Win32 console
  API *does* deliver `KEY_EVENT_RECORD`s for a bare modifier press and
  release (`VK_SHIFT`/`VK_CONTROL`/`VK_MENU`), and crossterm's Windows
  backend surfaces these as `KeyCode::Modifier(ModifierKeyCode::...)` events
  with `KeyEventKind::Press` / `KeyEventKind::Release` — **but only when
  crossterm's keyboard enhancement flags are pushed**
  (`PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)`).
  Without pushing that flag, Windows Terminal and conhost both report only
  `KeyEventKind::Press` for ordinary keys and **do not surface standalone
  modifier key events as distinct `Event::Key` items at all** — the
  modifier state only ever arrives as the `KeyModifiers` bitfield attached to
  some *other* key's event.
- **With `REPORT_EVENT_TYPES` pushed:** Windows Terminal (which implements
  enough of the Kitty protocol surface crossterm negotiates) does deliver
  separate press/release events for a bare modifier. Legacy `conhost.exe`
  does **not** implement the enhancement-flag query crossterm uses to detect
  support, so `crossterm::terminal::supports_keyboard_enhancement()` reports
  `false` there and the feature silently isn't available — this must be
  treated as expected, not a bug.
- Toggling this mode has a real cost: pushing keyboard enhancement flags
  changes how *all* key events are reported (e.g. release events start
  arriving for every key, not just modifiers), so the rest of the input
  pipeline (`filecommand-tui/src/input/mod.rs`, which currently assumes
  "one `Event::Key` with `KeyEventKind::Press` per keystroke") would need to
  filter on `KeyEventKind` everywhere, not just at the modifier-detection
  site.

## Conclusion for M1

Live F-key-bar relabeling on modifier press/release is **feasible on Windows
Terminal but not on legacy conhost**, and only after opting into keyboard
enhancement flags, which has pipeline-wide implications beyond the F-key bar
itself. M1 ships the F-key bar as fully static (per spec) and does **not**
push keyboard enhancement flags. A later milestone that wants live relabeling
should:

1. Gate the feature behind `supports_keyboard_enhancement()` at startup, with
   the static bar as the conhost/unsupported-terminal fallback.
2. Re-audit `input/mod.rs` for `KeyEventKind::Release`/`Repeat` handling
   before enabling `REPORT_EVENT_TYPES`, since that flag changes event
   delivery for every key, not just modifiers.
