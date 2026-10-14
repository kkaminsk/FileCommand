# Tasks: relative-destination-resolution

## 1. Shared resolver (`crates/filecommand-core/src/update.rs`)

- [x] 1.1 In `resolve_cd_target`, on Windows only, treat a target of the form `X:` followed by a non-separator character (`D:old`) as drive-relative. Join the rest to `cwd` when `cwd` is on drive `X` (case-insensitive, via `drives::drive_letter_of`), and to `drives::drive_root(X)` otherwise (design D2)
- [x] 1.2 Pass every `Some` result, joined or typed absolute, through `fs_ops::path::lexically_normalize`, and update the doc comment to the "absolute and normalized whenever `cwd` is" contract (design D2)
- [x] 1.3 Unit tests in `update/tests.rs`:
  - `C:\work\proj` + `..\archive` → `C:\work\archive`
  - `C:\work` + `backup` → `C:\work\backup`
  - `C:\work` + `\archive` → `C:\archive`
  - `D:\work` + `D:old` → `D:\work\old`
  - `C:\work` + `D:old` → `D:\old`
  - UNC and absolute pass through unchanged
  - `C:\` + `..` → `None`
  - The existing `cd_dotdot_and_bare_drive_letters_resolve` and `cd_accepts_a_manually_entered_unc_path` still pass
- [x] 1.4 Test that `cd ..\x` begins a listing of the normalized `C:\…\x` path (the `StartListing` path has no `..` component) (command-line: "cd with a parent-relative path lands on the normalized directory"; design D2)
- [x] 1.5 Test that `cd D:old` from `D:\work` targets `D:\work\old` and from `C:\work` targets `D:\old`, and that `del ..\old.txt` from `C:\work\proj` opens the delete confirmation for `C:\work\old.txt` (command-line: "cd with a drive-relative path resolves against the panel or the drive root", "del with a parent-relative path names the resolved file"). If `offload-blocking-io` has landed, assert on the probe request's path instead of the synchronous outcome

## 2. Destination dialog (`update.rs`)

- [x] 2.1 In `confirm_destination_input`, for Copy/Move, set `dest_dir` from `resolve_cd_target(&source_dir, trimmed)`. On `None`, return the unchanged `DestinationInput` phase, as for an empty field (operation-dialogs: "Relative destinations resolve against the source panel directory"; design D1)
- [x] 2.2 In `confirm_destination_input`, for Mkdir, resolve the same way and build `Job { dest_dir: target.parent(), new_dir_name: target.file_name() }`. Keep the dialog open when there is no parent or file name (design D3)
- [x] 2.3 In the `FileOpSetup::RenameInput` confirm arm, resolve the same way and build `Job { dest_dir: target.parent(), new_dir_name: target.file_name() }`. A plain name must yield exactly today's job (design D3)
- [x] 2.4 Update the `Job::dest_dir` / `new_dir_name` doc comments in `fs_ops/job.rs` (design D3)

## 3. RealFs invariant (`crates/filecommand-core/src/fs_ops/fs.rs`)

- [x] 3.1 Add a private `ext(path)` helper with `debug_assert!(path.is_absolute(), …)` that returns `to_extended_path(path)`. Route every `RealFs` method in both `windows_impl` and `portable_impl` through it. Leave `to_extended_path` itself unasserted (design D4)

## 4. Reducer tests (`crates/filecommand-core/src/update/tests.rs`)

Use Windows-style absolute panel paths, behind `#[cfg(windows)]` where `Path` semantics require it, so the drive forms are meaningful.

- [x] 4.1 F6 with source `C:\work`, field `backup`: `Effect::RunJob` carries `dest_dir == C:\work\backup` (scenario "A relative name lands inside the source panel's directory")
- [x] 4.2 `..\archive` from `C:\work\proj` gives `dest_dir == C:\work\archive`. With the other panel at `C:\work\archive`, a following `JobDone` re-reads that panel (scenario "A parent-relative path is resolved and normalized")
- [x] 4.3 `D:` gives `D:\`, and `\archive` gives `C:\archive` (scenarios "A bare drive letter means that drive's root", "A drive-rooted path keeps the source panel's drive")
- [x] 4.4 `D:old` from `D:\work` gives `D:\work\old`, and from `C:\work` gives `D:\old` (scenario "A drive-relative path resolves against the source panel or the drive root")
- [x] 4.5 `\\server\share\dest` and `E:\backup` are unchanged (scenario "UNC and absolute destinations are used unchanged")
- [x] 4.6 Drag from the inactive right panel (`D:\src`) to the left panel, edit the drop field to `sub`, `FileOpConfirmAs(Copy)`: `dest_dir == D:\src\sub` (scenario "An edited drop destination resolves against the drag's source panel")
- [x] 4.7 F7 `..\shared` from `C:\work\proj` gives `Job { dest_dir: C:\work, new_dir_name: shared }`, and `JobDone` re-reads a panel at `C:\work`. F7 `sub` yields exactly today's job (scenario "A make-directory name with a parent path creates it where it points")
- [x] 4.8 `..` from `C:\` keeps the `DestinationInput` phase with the text intact and emits no effects (scenario "An unresolvable destination keeps the dialog open")
- [x] 4.9 In-place Rename with a plain new name produces the same `Job` as before. `rename_case_only_edit_reaches_the_job_unchanged` still passes (design D3)
- [x] 4.10 Confirm the existing `destination_input_*`, `pre_filled_destination_accepts_unchanged_with_enter`, `file_action_menu_copy/move_*` and drop-dialog tests still pass. The `/left`/`/right` test paths compare equal after normalization because `Path` equality is component-wise

## 5. Verify

- [x] 5.1 `cargo test --workspace` green, including debug-build runs where the `RealFs` assertion is active
- [x] 5.2 `cargo clippy --workspace --all-targets` introduces no new warnings
- [x] 5.3 `openspec validate relative-destination-resolution --strict` clean
- [ ] 5.4 Manual: launch FileCommand from `C:\Users\<me>`, navigate the left panel to a scratch directory, and confirm each of the following:
  - F5 with `backup` copies into `<scratch>\backup`, not the launch directory.
  - F5 with `..\x` lands in the scratch directory's parent.
  - `cd ..\x` on the command line lists normally.
