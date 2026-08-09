# Tasks: purple-lights-file-contrast

## 1. Palette change

- [ ] 1.1 In `Theme::purple_lights()` set `Role::PanelFile` to ANSI `bright-black` on magenta and change its truecolor override to `Rgb(169, 169, 169)` on the existing `Rgb(48, 0, 64)` (theme-system: "purple-lights file rows render in dark grey")
- [ ] 1.2 Update `purple_lights_role_anchors_match_spec` (and any other test pinning `PanelFile`) to the new values (theme-system: "purple-lights file rows render in dark grey")

## 2. Verification

- [ ] 2.1 Refresh purple-lights snapshots; assert every non-purple-lights snapshot is byte-identical (theme-system: "purple-lights file rows render in dark grey")
- [ ] 2.2 Full `cargo build --workspace` and `cargo test --workspace` pass
