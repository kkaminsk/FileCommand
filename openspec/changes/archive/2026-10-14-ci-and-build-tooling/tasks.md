# Tasks: ci-and-build-tooling

Commit order matters (design D7): format, then baseline fixes, then manifests and FFI, then the workflow, then blame-ignore. Start this branch when few other `build/` branches are open (proposal: Coordination).

## 1. Formatting

- [x] 1.1 Add `rustfmt.toml` at the repo root with `max_width = 150` and `use_small_heuristics = "Max"`, and nothing else (build-tooling: "Formatting configuration is committed"; design D2)
- [x] 1.2 Run `cargo fmt --all` and commit the result **alone**, with no other edits in the commit. Expect about 195 hunks in about 44 files when starting from `1bada6f`, more if other branches have merged since (design D2)
- [x] 1.3 Verify that the reformat commit leaves `cargo test --workspace` green and the insta snapshots unchanged (build-tooling: "The reformat commit is isolated")

## 2. Clean baseline

- [x] 2.1 Fix every warning from `cargo clippy --workspace --all-targets` (26 in the audit, 29 distinct in the review re-run), among them `useless_vec`, `default_constructed_unit_structs`, `collapsible_str_replace`, `cmp_owned`, `io_other_error`, `field_reassign_with_default`, `items_after_test_module`, `empty_line_after_doc_comments`, and the test-only `unused_variables`. Box the large variant of `editor::LoadResult` for `large_enum_variant`. Allow `too_many_arguments` locally, with a reason comment, only where a renderer genuinely needs the inputs. `upper_case_acronyms` goes away in 3.6 (build-tooling: "Clean baseline"; design D7)
- [x] 2.2 Fix the 13 rustdoc warnings from `cargo doc --workspace --no-deps`:
  - correct the unresolved links (`Theme::resolve`, `close_tab`, `switch_tab`);
  - turn links to private items (`update_impl`, `drag_allowed`, `State::next_request_id`, `Injection`, and the rest) into plain code spans;
  - wrap `<path>` and `<year>` in backticks;
  - disambiguate `update` with `fn@` or `mod@`.

  (build-tooling: "A broken doc link fails the pull request"; design D7)
- [x] 2.3 Confirm that `cargo clippy --workspace --all-targets -- -D warnings` and `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` both pass locally

## 3. Manifests, dependencies, and FFI

- [x] 3.1 Workspace `Cargo.toml`: add `rust-version` to `[workspace.package]`, and add `rust-version.workspace = true` to both crate manifests. Set the value by running `cargo +1.88 check --workspace --all-targets --locked` and raising it to the first toolchain that passes (build-tooling: "Toolchain floor and release profile are declared"; design D3)
- [x] 3.2 Workspace `Cargo.toml`: add `[profile.release]` with `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`, and a comment explaining why `panic` stays `"unwind"` (design D4)
- [x] 3.3 Change `git2` in `[workspace.dependencies]` to `{ version = "0.19", default-features = false }` and refresh `Cargo.lock`. Confirm that `cargo tree -p filecommand-core` shows no `libssh2-sys`, `openssl-sys`, or `openssl-probe`. Check that `crates/filecommand-core/build.rs` is still needed: remove its `advapi32` line once, confirm `cargo test` fails to link, restore it, and update its comment to say the link list of `libgit2-sys 0.17` does not depend on features (build-tooling: "libgit2 is built with only the features local status needs"; design D5)
- [x] 3.4 Move `windows-sys` to `[workspace.dependencies]` at `0.59` with the union of features (`Win32_Foundation`, `Win32_Storage_FileSystem`, `Win32_Security` (required for `CreateFileW`), `Win32_System_Diagnostics_Debug`, `Win32_System_SystemInformation`, `Win32_UI_Shell`, `Win32_UI_WindowsAndMessaging`, plus any feature other landed changes have added, e.g. `Win32_System_Console`, `Win32_System_Com`). Use it from `[target.'cfg(windows)'.dependencies]` in both crates; if `editor-encoding-safety` already added a core `windows-sys` entry, convert that entry instead of adding a second one (design D6)
  - Scope note: this task was designed against `build/open-proposals-2026-10` (`1bada6f`). On the current branch the TUI contains no `ShellExecuteW`/`windows-sys` FFI yet — that code arrives via the sibling change `open-with-associated-app` (and `offload-blocking-io` adds the COM init). The TUI part of this task applies only if one of those has landed first; otherwise there is nothing to migrate and the task is satisfied by the other `extern "system"` migrations in this section (3.5 `crates/filecommand-core/src/drives.rs:145`, 3.6 `fs_ops/fs.rs:442` and `:518`, 3.7 `crates/filecommand-tui/src/clock.rs:63`). See design: Coordination.
- [x] 3.5 `crates/filecommand-core/src/drives.rs`: replace the `extern "system"` block and `MemoryStatusEx` with `windows-sys` (`GetLogicalDrives`, `SetThreadErrorMode`, `SEM_FAILCRITICALERRORS`, `GetVolumeInformationW`, `GetDiskFreeSpaceExW`, `GlobalMemoryStatusEx`, `MEMORYSTATUSEX`), keeping identical arguments and buffer sizes (build-tooling: "Windows FFI uses windows-sys and every unsafe block documents its invariant")
- [x] 3.6 `crates/filecommand-core/src/fs_ops/fs.rs`: replace the declarations in `file_id_via_handle` (`CreateFileW`, `GetFileInformationByHandle`, `CloseHandle`, `BY_HANDLE_FILE_INFORMATION`, the share and flag constants, `INVALID_HANDLE_VALUE`) and in `stream_enum` (`FindFirstStreamW`, `FindNextStreamW`, `FindClose`, `GetLastError`, `WIN32_FIND_STREAM_DATA`, `FindStreamInfoStandard`, the error constants). Delete the hand-written `HANDLE`, `c_void`, `FileTime`, and struct copies (design D6)
- [x] 3.7 `crates/filecommand-tui/src/clock.rs`: replace `GetLocalTime` and the hand-written `SystemTime` with `windows-sys` `GetLocalTime` and `SYSTEMTIME` (design D6)
- [x] 3.8 Add a `// SAFETY:` comment, stating the invariant from the audit appendix, to every remaining `unsafe` block in `drives.rs`, `fs_ops/fs.rs`, and `clock.rs`. Keep `app.rs`'s existing one. If `viewer-mapping-safety` has not landed yet, give `viewer/byte_source.rs`'s `Mmap::map` block an interim SAFETY comment stating its platform limits (design D6; proposal: Coordination)
- [x] 3.9 Add `[workspace.lints.clippy] undocumented_unsafe_blocks = "warn"` to the workspace and `[lints] workspace = true` to both crates, then confirm clippy is clean (build-tooling: "Undocumented unsafe fails CI")
- [x] 3.10 Confirm that a search of the workspace for `extern "system" {` declaration blocks and Windows-mirroring `#[repr(C)]` structs finds nothing. Any `extern "system" fn` left is a callback (e.g. a console control handler or a `CopyFileExW` progress routine) whose signature matches its `windows-sys` function-pointer type. If `durable-file-copy` landed first with a hand-declared `CopyFileExW`, move it to `windows-sys` here (build-tooling: "No hand-declared Windows prototypes")

## 4. Behavior checks for the dependency and FFI changes

- [x] 4.1 Existing `git_info` tests pass unchanged (branch, modified, staged, untracked, pathspec scoping) (build-tooling: "Git info still works")
- [x] 4.2 Existing drive, volume, and info tests pass. Manual on Windows: Alt+F1 lists drives with labels, and the Info panel shows free space, the serial, and available memory (build-tooling: "Behavior is unchanged")
- [x] 4.3 Existing `fs_ops` identity and ADS tests pass. Manual on Windows: copy a file with a `Zone.Identifier` stream and confirm the stream is copied, then do a case-only rename (build-tooling: "Behavior is unchanged")
- [x] 4.4 Build `--release`, run `cargo test --release -p filecommand-tui --test panic_restoration`, and confirm that `filecommand.exe` is smaller than before the profile change (build-tooling: "Panics still unwind in release builds", "Release binary is optimized and stripped")

## 5. CI workflow

- [x] 5.1 Add `.github/workflows/ci.yml` with three jobs (build-tooling: "Continuous integration gates every pull request"; design D1):
  - `windows` on `windows-latest`, pinned stable toolchain with `rustfmt` and `clippy`, cargo cache, running fmt, then clippy `-D warnings`, then test, then doc with `RUSTDOCFLAGS=-D warnings`, all with `--locked`;
  - `msrv` on `windows-latest`, `cargo check --workspace --all-targets --locked` on the `rust-version` toolchain;
  - `linux` on `ubuntu-latest` with `continue-on-error: true`, build and test `--locked`.
- [x] 5.2 Trigger the workflow on `pull_request` and on `push` to `main`, and pin the stable toolchain version in a single workflow-level variable (design D1)
- [ ] 5.3 Open the PR and confirm the first run is green on `windows` and `msrv`. Record the `linux` result in the PR description, whatever it is (build-tooling: "Clean baseline", "Linux results never block a merge")
- [ ] 5.4 Manual (user): after the first green run, enable branch protection on `main` requiring the `windows` and `msrv` checks (design: Migration Plan)

## 6. Blame and docs

- [x] 6.1 Add `.git-blame-ignore-revs` listing the reformat commit's full hash, with a comment line naming the change (build-tooling: "Formatting configuration is committed")
- [x] 6.2 README "Building from source": name the minimum Rust version, and note that non-Windows builds are best-effort and need a C compiler but no OpenSSL headers (build-tooling: "Toolchain floor and release profile are declared"; design D8)
- [x] 6.3 README "Testing": replace "file ops against temp directories" with an accurate description (FakeFs-based engine tests, git-info tests on temporary repositories, TUI snapshot tests, plus the `RealFs` suite only if `durable-file-copy` has landed). List the four commands of the required CI job (build-tooling: "Continuous integration gates every pull request"; design D8)

## 7. Verify

- [x] 7.1 `cargo fmt --all -- --check` clean
- [x] 7.2 `cargo clippy --workspace --all-targets -- -D warnings` clean
- [x] 7.3 `cargo test --workspace` green
- [x] 7.4 `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` clean
- [x] 7.5 `openspec validate ci-and-build-tooling --strict` clean
