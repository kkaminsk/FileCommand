# Design: hidden-entry-styling

## Context

`listing.rs:438-451` builds `RawDirEntry{name,is_dir,size,modified}` from `entry.metadata()`; converting to `Entry` at `:455`. `theme.rs` roles are an enum + `ALL_ROLES`; `Theme::build` rejects a missing role. `panel.rs` chooses a row style in Full mode as drag target → cursor → directory/file (`:596-604`), with the selected style applied by the name-cell styling; the Brief renderer (`:709-719`) consults the selection inline in the same drag → cursor → selected → directory/file order.

## Goals / Non-Goals

**Goals:**

- Hidden/system entries recognizable at a glance in every theme.
- Zero behavioral change beyond color.
- Attribute read from the enumeration (no per-file stat).

**Non-Goals:**

- A show/hide toggle.
- Attribute letters in the mini-status.
- Tree mode and Quick view styling.
- An Attributes dialog.

## Decisions

### D1: `hidden` is computed in the reader, once

Windows: `metadata.file_attributes() & (0x2 | 0x4) != 0`. Non-Windows: `name` begins with `.`. Stored on `RawDirEntry` and copied to `Entry`, so sorting/filtering code never recomputes it. The fake readers used by tests gain the field with a default of `false`.

Alternative rejected: computing it lazily in the renderer from the name — the renderer has no attribute access, and the dot-prefix rule is wrong on Windows.

### D2: One new role, dimmer than `panel.file` on the same background

| Theme | `panel.hidden` ANSI fg / bg | Truecolor fg (bg as the theme's file bg) |
|---|---|---|
| nc-classic | bright-black / blue | none |
| nc-mono | bright-black / black | none |
| terminal-green | bright-black / black | `#1E7A1E` |
| purple-lights | blue / magenta | `#6E5A80` on `#300040` |
| yellow-storm | bright-black / black | `#806000` |
| inverted | bright-black / bright-white | none |

Each pair must remain legible but visibly subordinate to `panel.file`; the existing theme contrast tests are extended to assert `panel.hidden != panel.file` in every built-in theme.

### D3: Precedence is a single ordered match

`style = drag_target ?? cursor ?? selected ?? (hidden → panel.hidden) ?? (dir → panel.directory | panel.file)`. Hidden directories lose their bright-white directory style in favor of `panel.hidden` (uppercase/marker glyphs unchanged), so `.git` reads as "hidden" rather than "important". The git marker column keeps its own roles.

## Risks / Trade-offs

- [`bright-black` on `blue` (nc-classic) is low contrast on some palettes] → That is the intended "dimmed" look; the value can be tuned per theme without renderer changes.
- [Adding a role grows every theme table by one line] → No user theme loader exists today, so no external file breaks.

## Open Questions

- None.
