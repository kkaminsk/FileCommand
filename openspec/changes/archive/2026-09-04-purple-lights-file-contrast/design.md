# Design: purple-lights-file-contrast

## Context

`Theme::purple_lights()` (`filecommand-core/src/theme.rs`) sets `Role::PanelFile` to bright-magenta on magenta with a truecolor override of `Rgb(186, 85, 211)` on `Rgb(48, 0, 64)`. The visual-themes spec anchors purple-lights' backdrop, frame, directory, cursor, and keybar colors but never `panel.file`, and the theme's own code comment describes it as "nc-classic's structure with magenta standing in for blue and bright-magenta for cyan" — file rows inherited the frame's color, which is why they blend into the chrome. The color-depth policy requires a mandatory ANSI-16 named value with an optional truecolor override.

## Goals / Non-Goals

**Goals:**

- File rows clearly readable against the purple backdrop in the truecolor rendition, and rendered in the palette's dark grey in ANSI-16, per the user's requested design.
- Zero change to any other role or theme.

**Non-Goals:**

- No re-balancing of the rest of the purple-lights palette; no contrast work in other themes; no user-configurable colors.

## Decisions

### D1: Truecolor `#A9A9A9` on the existing `#300040` backdrop

`#A9A9A9` is the canonical "dark grey" (CSS DarkGray) and yields roughly 7:1 contrast against `#300040` — comfortably readable — while staying visibly muted next to bright-white directories, preserving the file-vs-directory hierarchy. A literal darker grey (`#696969`) was rejected: at ~3:1 it would trade one readability problem for another; `#A9A9A9` honors the requested color family at the shade that actually fixes the complaint.

### D2: ANSI-16 fallback is `bright-black`

`bright-black` is the 16-color palette's only dark grey, so it is the faithful mandatory fallback for the requested design. Trade-off accepted: on some ANSI palettes bright-black over magenta measures slightly lower contrast than today's bright-magenta; the user's terminals (Windows Terminal, conhost) report truecolor, where the `#A9A9A9` override governs. Rejected alternative: `white` as the fallback — more contrast, but it is not the requested design and would collide with `screen.placeholder`'s white.

### D3: Update the role-anchor test alongside

`purple_lights_role_anchors_match_spec` in `theme.rs` asserts palette anchors; it gains/updates a `PanelFile` assertion for the new values so the spec anchor added here is pinned by test, matching how the existing anchors are covered.

## Risks / Trade-offs

- [ANSI-16 bright-black on magenta can look dim on some palettes] → accepted deliberately (D2); the truecolor path is the one in actual use, and the fallback remains the palette's only faithful dark grey.
- [Snapshot churn in purple-lights renders] → refresh the affected `.snap` files; all other themes' snapshots must stay byte-identical.

## Open Questions

- None. The color request came directly from the user; shade selection is resolved in D1/D2.
