# Change: ci-and-build-tooling

## Why

FileCommand is built mostly in agent-driven batches on `build/<batch>` branches, and nothing mechanical checks a branch before it reaches `main`. The 2026-10-12 audit scored Tooling/CI 1 out of 5:

- **B-03 (Medium): no CI, failing format check, no lint gates.**
  - The repository has no `.github/` workflow, no `rustfmt.toml`, no crate lint configuration, no `rust-version`, and no `[profile.release]`.
  - `cargo fmt --all -- --check` fails with 2,151 hunks in 86 files, because the code is written to about 150 columns with a formatter configuration that was never committed.
  - Clippy reports 26 warnings.
  - A PR that breaks the build or the tests can be merged without anyone noticing, and every contributor or agent run reformats code differently.
- **B-04 (Medium): oversized `git2`.** `git2` is built with its default features (SSH, HTTPS), although `git_info.rs` only uses `Repository::discover`, `head()`, and `statuses()`. This compiles libssh2 from C on every clean build. On Linux it also pulls in `openssl-sys`, which needs OpenSSL development headers, so the "best-effort" Linux build *fails* on a stock machine.
- **B-07 (Low): hand-written Windows FFI.** Core hand-declares `extern "system"` prototypes and struct layouts for `CreateFileW`, `GetFileInformationByHandle`, `FindFirstStreamW`, `GetVolumeInformationW`, and others, although `windows-sys` is already in the lockfile and used by the TUI. One wrong field type would be undefined behavior with no help from the compiler. Only 1 of the 11 `unsafe` blocks has a `// SAFETY:` comment.
- **B-09 (Low, rustdoc and README part): stale documentation.** `cargo doc` reports 13 warnings: broken links, links to private items, unclosed HTML tags, and an ambiguous link. The README's Testing section says file operations are tested "against temp directories". That is not true of the real filesystem engine.

## What Changes

- **CI workflow.** A GitHub Actions workflow runs on every pull request and every push to `main`.
  - Required job on `windows-latest`: format check, clippy with `-D warnings` on all targets, the full test suite, and rustdoc with `-D warnings`.
  - Required job: `cargo check` on the declared minimum toolchain.
  - Best-effort job on `ubuntu-latest`, allowed to fail without blocking.
- **Committed formatting configuration.** A `rustfmt.toml` that reproduces the existing style as closely as rustfmt allows (`max_width = 150`, `use_small_heuristics = "Max"`). A one-time `cargo fmt --all` follows as its own commit with no logic changes, and it is recorded in `.git-blame-ignore-revs`.
- **Toolchain floor and release profile.**
  - `rust-version` is set in `[workspace.package]` and inherited by both crates.
  - `[profile.release]` sets `lto = "thin"`, `codegen-units = 1`, and `strip = "symbols"`.
  - `panic` stays `"unwind"`, because worker-thread isolation and the terminal-restoring guard rely on unwinding.
- **Trimmed `git2`.** The dependency is declared with `default-features = false`. libssh2 and, on Unix, OpenSSL drop out of the build, so Linux no longer needs OpenSSL headers. The `advapi32` link workaround in `crates/filecommand-core/build.rs` stays, because `libgit2-sys` still does not declare it.
- **Windows FFI through `windows-sys`.** Core's hand-declared prototypes and layouts (`drives.rs`, `fs_ops/fs.rs`) and the TUI's `GetLocalTime` (`clock.rs`) move to `windows-sys` as a Windows-only dependency. Every `unsafe` block gets a `// SAFETY:` comment, and a workspace lint (`clippy::undocumented_unsafe_blocks`) keeps it that way.
- **Clean baseline.** Fix every clippy warning (26 by the audit's count) and the 13 rustdoc warnings so the new gates start green. Correct the README's Testing claim, and document the toolchain floor and the Linux build in "Building from source".

## Capabilities

### New Capabilities

- `build-tooling`: how the repository is checked and built. It covers the CI gates on pull requests, the committed formatter configuration, the declared toolchain floor and release profile, the `git2` feature set, and the rule that Windows FFI goes through `windows-sys` with a documented invariant on every `unsafe` block. Installer packaging stays in `windows-installer` and is not duplicated here.

### Modified Capabilities

None.

## Impact

- **New files:**
  - `.github/workflows/ci.yml`
  - `rustfmt.toml`
  - `.git-blame-ignore-revs`
- **`Cargo.toml` (workspace):**
  - `[workspace.package] rust-version`
  - `[profile.release]`
  - `[workspace.dependencies]`: `git2 = { version = "0.19", default-features = false }` and `windows-sys = { version = "0.59", features = [...] }`
  - `[workspace.lints.clippy] undocumented_unsafe_blocks = "warn"`
- **Crate manifests:** `crates/filecommand-core/Cargo.toml` adds `rust-version.workspace`, `[lints] workspace = true`, and `[target.'cfg(windows)'.dependencies] windows-sys`. `crates/filecommand-tui/Cargo.toml` adds the same two keys, and its `windows-sys` entry switches to the workspace dependency.
- **`Cargo.lock`:** `libssh2-sys`, `openssl-sys`, and `openssl-probe` are no longer reachable.
- **Windows FFI:**
  - `crates/filecommand-core/src/drives.rs` (`platform` module: `GetLogicalDrives`, `SetThreadErrorMode`, `GetVolumeInformationW`, `GetDiskFreeSpaceExW`, `GlobalMemoryStatusEx`, and the hand-written `MemoryStatusEx`)
  - `crates/filecommand-core/src/fs_ops/fs.rs` (`file_id_via_handle`'s `CreateFileW`, `GetFileInformationByHandle`, and `CloseHandle`, the hand-written `ByHandleFileInformation` and `HANDLE`, and `stream_enum`'s `FindFirstStreamW`, `FindNextStreamW`, `FindClose`, and `GetLastError` with `Win32FindStreamData`)
  - `crates/filecommand-tui/src/clock.rs` (`GetLocalTime` and `SystemTime`)
  - `crates/filecommand-tui/src/app.rs` (`ShellExecuteW`, which already has a SAFETY comment)
- **Code formatting:** every `.rs` file is reformatted once, in a dedicated commit.
- **Warning fixes:**
  - The clippy warnings are spread across both crates. Among them are `useless_vec`, `cmp_owned`, `large_enum_variant` on `editor::LoadResult`, `too_many_arguments`, and `upper_case_acronyms` on the hand-written `HANDLE`, which disappears with `windows-sys`.
  - The rustdoc warnings include the links to `Theme::resolve`, `close_tab`, `switch_tab`, `update_impl`, `drag_allowed`, `State::next_request_id`, and `Injection`, the `<path>` and `<year>` tags, and the ambiguous `update`.
- **`README.md`:** "Building from source" (toolchain floor, Linux notes) and "Testing" (accurate scope, local CI commands).
- **Not touched:** `installer/` (covered by `windows-installer`).

**Audit findings:** B-03, B-04, B-07, B-09 (rustdoc and README-accuracy parts only)

**Coordination:**
- **Landing order.** Every other audit proposal touches Rust sources, so the one-time `cargo fmt` commit conflicts with any `build/` branch open at the same time. Land this change, or at least its format commit, **when few build branches are open**. A good point is right after the current Critical and High batch merges, before the Low and refactor batch starts. Branches opened later start from formatted code.
- `durable-file-copy` (R-07, R-08, R-14, B-01) adds the `RealFs` test suite on temporary directories. The README Testing claim is corrected here to describe whatever suites exist when this change lands. If `durable-file-copy` has already landed, the "file operations against temp directories" wording becomes true and stays.
- **Windows FFI added by other changes.** `durable-file-copy` (R-14) adds `CopyFileExW` and a progress-routine callback in core, hand-declared if it lands first. Whichever of the two lands second moves that prototype to `windows-sys` (`Win32_Storage_FileSystem`). `editor-encoding-safety` (R-02) adds a core `windows-sys` entry for `ReplaceFileW`; this change converts that entry to the workspace dependency rather than adding a second one. `process-lifecycle-hardening` (R-10, `Win32_System_Console`) and `offload-blocking-io` (P-09, `Win32_System_Com`) add TUI features, which join the workspace feature union. Callback definitions such as the console control handler are allowed under the no-hand-declared-prototypes rule.
- `viewer-mapping-safety` (R-20) removes `memmap2` and the `unsafe { Mmap::map }` block. If it lands first, this change has one `unsafe` block fewer to document. If this change lands first, the block gets an interim SAFETY comment.
- `streaming-listing-performance` (P-01 to P-04) owns refreshing the 100k-entry spike document (`docs/spikes/c-100k-entry-render-benchmark.md`), the other part of B-09. It is not touched here.
- `config-parsing-robustness` (R-15, R-16, B-05) owns the misordered `[mouse]` example in the README configuration section. It is not touched here.
- `editor-encoding-safety` (R-02) reshapes editor loading. The clippy `large_enum_variant` fix on `editor::LoadResult` must be rebased onto whichever of the two lands second.
- `reducer-overlay-structure` (B-02, B-08) moves code out of `update.rs`, which contains the private items that two of the rustdoc warnings point at (`update_impl`, `drag_allowed`). Whichever lands second re-checks `cargo doc` under `-D warnings`. CI then enforces it.
