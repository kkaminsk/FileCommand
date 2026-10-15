# Design: ci-and-build-tooling

## Context

Verified on `build/open-proposals-2026-10` (`1bada6f`) unless noted.

**Repository tooling today.**
- The repository root has no `.github/`, `rustfmt.toml`, `clippy.toml`, `rust-toolchain.toml`, or `.gitattributes`.
- The workspace `Cargo.toml` has `[workspace.package]` (version 0.1.4, edition 2021) and `[workspace.dependencies]`, but no `rust-version`, no `[profile.*]`, and no `[workspace.lints]`.
- `Cargo.lock` is committed.
- The audit environment was `rustc 1.98.1`.

**Formatting: what config reproduces the code.** I exported the build branch's `crates/` tree with `git archive` into a scratch directory and ran `rustfmt 1.9.0 --check --edition 2021` with candidate configurations:

| Configuration | Diff hunks | Files with diffs |
|---|---|---|
| defaults (`max_width = 100`) | 2,151 | 86 (matches the audit) |
| `max_width = 150` | 1,142 | — |
| `max_width = 140`, `use_small_heuristics = "Max"` | 242 | — |
| **`max_width = 150`, `use_small_heuristics = "Max"`** | **195** | **44** |
| `max_width = 160`, `use_small_heuristics = "Max"` | 231 | — |

Line lengths agree. `update.rs` peaks at 148 columns, `panel.rs` at 150, and `app.rs` at 152 (one line). Across all `.rs` files, 1,997 lines exceed 100 columns but only 57 exceed 150. Most of those are long string literals and test assertions, which rustfmt does not break.

The audit's guess of `max_width = 150` is therefore right, but it is not enough by itself. The code also packs struct literals and call arguments up to the full width, which is `use_small_heuristics = "Max"`.

The remaining 195 hunks come from rules rustfmt does not let you configure. An example: if one struct-like variant of an enum spans several lines, rustfmt formats every struct-like variant of that enum on several lines. This affects `UiPhase` and `Command`. No configuration reaches zero diffs, so a one-time format commit is unavoidable. With this configuration it is 195 hunks instead of 2,151.

**Lint state.** The audit counted 26 clippy warnings and no errors, all low-severity. A review re-run of `cargo clippy --workspace --all-targets -- -W clippy::all` on `1bada6f` (rustc 1.98.1) printed 29 distinct warnings, among them `useless_vec` ×4, `cmp_owned` ×3, `unnecessary_lazy_evaluations` ×2, `collapsible_if` ×2, and `upper_case_acronyms` ×2 (one for each of the two hand-written `HANDLE` aliases in `fs_ops/fs.rs`). The exact count does not matter: the baseline fixes every warning the gate's own command reports. `cargo doc --workspace --no-deps` reports 13 warnings, all in `filecommand-core` (confirmed by the same re-run): 3 unresolved links, 7 links to private items, 2 unclosed HTML tags (`<path>`, `<year>`), and 1 ambiguous link (`update`, function vs module).

**`git2` and its features.** `git2 = "0.19"` is used with default features `["ssh", "https", "ssh_key_from_memory"]`. `crates/filecommand-core/src/git_info.rs` uses only `Repository::discover`, `Repository::head().shorthand()`, and `Repository::statuses` with `StatusOptions` (plus `Repository::init` in tests). None of these needs a network transport.

From the registry sources of `git2 0.19.0` and `libgit2-sys 0.17.0+1.8.1`:
- `ssh` pulls in `libssh2-sys`, a C build.
- `https` pulls in `openssl-sys` and `openssl-probe`, on Unix only (`git2` declares both for Unix other than macOS; `libgit2-sys` declares `openssl-sys` for every Unix). On Windows `https` uses WinHTTP and adds no crate.
- **`url` is an unconditional dependency of `git2 0.19`**, and `libz-sys` is an unconditional dependency of `libgit2-sys`.

**`build.rs`.** `crates/filecommand-core/build.rs` emits `cargo:rustc-link-lib=advapi32` on Windows. `libgit2-sys 0.17`'s build script links `winhttp`, `rpcrt4`, `ole32`, `crypt32`, and `secur32` on Windows. That list does not depend on any feature and does not include `advapi32`. The libgit2 sources that need `advapi32` (`fs_path.c`, `sysdir.c`, `rand.c`) are compiled whatever the features are. Trimming features therefore does not make the workaround unnecessary.

**Windows FFI and `unsafe`.**
- `windows-sys 0.59.0` is in `Cargo.lock` and is a direct Windows dependency of `filecommand-tui` (features `Win32_UI_Shell`, `Win32_Foundation`, `Win32_UI_WindowsAndMessaging`), used for `ShellExecuteW`. `windows-sys 0.61.2` is also locked, through newer transitive crates.
- Core has no `windows-sys` dependency. It declares `#[link(name = "kernel32")] extern "system"` blocks in `drives.rs` (`GetLogicalDrives`, `SetThreadErrorMode`, `GetVolumeInformationW`, `GetDiskFreeSpaceExW`, `GlobalMemoryStatusEx` with a hand-written `MemoryStatusEx`) and in `fs_ops/fs.rs`:
  - `file_id_via_handle` declares `CreateFileW`, `GetFileInformationByHandle`, and `CloseHandle`, with hand-written `ByHandleFileInformation`, `FileTime`, and `HANDLE`, the last of which triggers clippy `upper_case_acronyms`.
  - `stream_enum` declares `FindFirstStreamW`, `FindNextStreamW`, `FindClose`, and `GetLastError`, with a hand-written `Win32FindStreamData` (296 `u16`s).
- The TUI declares `GetLocalTime` with a hand-written `SystemTime` (`clock.rs`).
- There are 11 `unsafe` blocks in the workspace:
  - 6 in `drives.rs`
  - 2 in `fs_ops/fs.rs`
  - 1 in `viewer/byte_source.rs`
  - 1 in `clock.rs`
  - 1 in `app.rs`, the only one with a `// SAFETY:` comment.

**README.**
- "Building from source" says only "Requires the Rust toolchain (stable)".
- "Testing" (`README.md:601-611`) says core tests cover "file ops against temp directories". The engine's tests all run against `FakeFs` (audit B-01).

**Panic strategy.** There is no `panic` setting, so release builds use the default, `unwind`. Two things depend on unwinding:
- The terminal is restored by an RAII guard's `Drop` and by a chained panic hook (`terminal.rs`).
- Worker threads (listing, jobs, queries) are plain `std::thread`s whose panics today end only that thread.

## Goals / Non-Goals

**Goals:**

- Every pull request is checked mechanically on Windows: format, lints, tests, docs, and the declared toolchain floor.
- One committed formatter configuration that matches the existing style, applied once, with the noise of that commit isolated.
- A smaller, faster release binary without changing panic behavior.
- A Linux build that works on a stock machine, best-effort, with no OpenSSL headers.
- No hand-written Windows struct layouts, and a documented invariant on every `unsafe` block, enforced by a lint.
- Docs that build without warnings, and a README that does not claim tests that do not exist.

**Non-Goals:**

- Replacing `git2` with `gix`, or removing the `url`/ICU dependency chain. `url` is not optional in `git2 0.19` (see Context), so feature trimming cannot remove it.
- The 100k-entry spike document refresh, which is `streaming-listing-performance`. The README `[mouse]` example order, which is `config-parsing-robustness`.
- Adding the `RealFs` test suite, which is `durable-file-copy` (B-01). Moving `FakeFs` behind `cfg(test)` also belongs there.
- Adding new tests beyond what the gates need.
- `cargo audit` or `cargo deny`, coverage, release automation, code signing in CI, or building the installer in CI. Installer packaging stays in `windows-installer`.
- Committing a `rust-toolchain.toml` to pin contributors' local toolchains.
- Changing the source-formatting conventions themselves beyond what the chosen configuration produces.

## Decisions

### D1: GitHub Actions workflow, Windows required, Linux best-effort

`.github/workflows/ci.yml` runs on `pull_request` (any target branch) and on `push` to `main`. It has three jobs:

1. **`windows`** (required), on `windows-latest`. It runs, in order:
   - `cargo fmt --all -- --check`
   - `cargo clippy --workspace --all-targets --locked -- -D warnings`
   - `cargo test --workspace --locked`
   - `cargo doc --workspace --no-deps --locked` with `RUSTDOCFLAGS="-D warnings"`

   Fast checks run first so a formatting slip fails in seconds.
2. **`msrv`** (required), on `windows-latest`: `cargo check --workspace --all-targets --locked` on the `rust-version` toolchain (D3).
3. **`linux`** (best-effort), on `ubuntu-latest` with `continue-on-error: true`: `cargo build --workspace --locked` and `cargo test --workspace --locked`. It reports results but never blocks a merge. Non-Windows behavior is best-effort per the project charter. This is the first time anything compiles the `cfg(not(windows))` paths.

Every job uses `--locked`, so CI tests the committed `Cargo.lock`. The toolchain is installed with a toolchain action and cached with a cargo cache action.

"Required" means a branch-protection rule on `main` that lists the `windows` and `msrv` checks. That is a repository setting the user applies. It is not a file in the repo, and tasks.md lists it as a manual step.

CI does not change the Git workflow. Nothing reaches `main` without the user's explicit confirmation (CLAUDE.md). CI adds a precondition; it grants no permission.

*Alternative considered: a floating `stable` toolchain for the lint job.* A new Rust release can add clippy lints that turn `-D warnings` red on an unchanged branch, in the middle of an agent batch. **Rejected.** The `windows` job instead pins the stable version it runs on in one workflow variable (initially the version the clean baseline was verified with). It is bumped by a deliberate PR that also fixes any new lints.

*Alternative considered: Linux as a required job.* The non-Windows code has never been compiled in CI (audit §9), and the charter calls it best-effort. **Rejected for now.** It can be promoted once it has been green for a while.

### D2: `rustfmt.toml` = `max_width = 150` plus `use_small_heuristics = "Max"`, then one format commit

Both options are stable rustfmt options. Measured against the current code (Context), this pair leaves 195 hunks in 44 files, against 2,151 in 86 at the defaults. That is the closest stable configuration found. `newline_style` stays at its default (`Auto`), so working trees checked out with `core.autocrlf = true`, the local setting on the author's machine, pass the check unchanged.

The reformat lands as **one commit with no logic changes**, made by running `cargo fmt --all` and nothing else. Its hash is then added to a new `.git-blame-ignore-revs`, so `git blame` (and GitHub's blame view) skips it. The commit goes on this change's `build/` branch ahead of the CI workflow commit, so the first CI run is green.

*Alternative considered: `max_width = 150` alone.* The audit suggested it. It leaves 1,142 hunks, because the code packs struct literals and arguments to full width. **Rejected.**

*Alternative considered: keep the default width and reformat everything.* That is 2,151 hunks, and every line wider than 100 columns would wrap, which conflicts with the evident house style. **Rejected.**

### D3: `rust-version` = the lowest toolchain that builds the locked workspace, verified by CI

The floor goes in `[workspace.package] rust-version` and both crates inherit it with `rust-version.workspace = true`.

The value is determined empirically during implementation, not guessed. The locked dependencies already require at least **1.88**: `instability 0.3.13`, reached through `ratatui`, and `darling 0.24` declare `rust-version = "1.88"`. The ICU 2.2 crates declare 1.86. Two dev-dependencies, `getrandom 0.4.3` and `idna_adapter 1.2.2`, use edition 2024. Start at 1.88 and run `cargo +1.88 check --workspace --all-targets --locked`. If the workspace's own code uses a newer standard-library API, raise the floor to the first version that passes. The `msrv` CI job (D1) keeps the declaration honest from then on.

### D4: `[profile.release]` with thin LTO, one codegen unit, stripped symbols, and `panic = "unwind"`

```toml
[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
# panic stays "unwind" (the default): see design D4.
```

Thin LTO and a single codegen unit give a smaller, faster binary at a modest cost in release build time; on Windows they are where the size gain comes from. `strip = "symbols"` matters mostly for the best-effort ELF and Mach-O builds, where it drops the symbol table. An MSVC `.exe` carries no COFF symbol table to begin with, and its debug info lives in the separate `.pdb`. Since Rust 1.79, `strip` no longer suppresses that `.pdb` on MSVC (rustc book, `-C strip`), and the installer does not ship it.

`panic` stays `"unwind"`, with a comment saying why:
- Under `abort`, a panic on any worker thread would kill the whole process instead of ending that thread.
- The terminal-restoring `Drop` guard would never run on the main thread.
- `process-lifecycle-hardening` (R-12) relies on `catch_unwind` in worker bodies.

The release-profile change is checked by building `--release` and running the `panic_restoration` integration test (task 4.4).

*Alternative considered: `lto = "fat"`.* It gives a marginally smaller binary at a noticeably slower build. Thin LTO with one codegen unit captures most of the gain. **Rejected.**

### D5: `git2` with `default-features = false`; keep the `advapi32` workaround

`[workspace.dependencies] git2 = { version = "0.19", default-features = false }`. This removes:
- `libssh2-sys` on every platform, a C build;
- `openssl-sys` and `openssl-probe` on Unix, which is what made Linux builds need OpenSSL headers.

It does **not** remove `url`/`idna`/ICU or `libz-sys`. Both are unconditional (Context). This corrects the audit, which listed URL/ICU among the default-feature costs.

The `git_info` tests, which create real repositories with `Repository::init`, and the git-info behavior are unchanged, because no remote operation is used.

`build.rs` stays. Its comment is updated to say the link list of `libgit2-sys 0.17` does not depend on features, so the workaround is needed with or without `default-features`. As a check, task 3.3 removes the `advapi32` line once on the build branch and confirms that `cargo test` fails to link without it, then restores it.

### D6: Windows FFI through `windows-sys`, SAFETY comments, and a lint

- `windows-sys` moves to `[workspace.dependencies]` at `0.59`, the version already locked for the TUI, so both crates share one copy. The feature list is the union of what both crates need:
  - `Win32_Foundation`, which is already used (`CloseHandle`, `GetLastError`, `INVALID_HANDLE_VALUE`, `FILETIME`, `SYSTEMTIME`);
  - `Win32_Storage_FileSystem`, for `CreateFileW`, `GetFileInformationByHandle`, `FindFirstStreamW`, `FindNextStreamW`, `FindClose`, `GetLogicalDrives`, `GetVolumeInformationW`, and `GetDiskFreeSpaceExW`;
  - `Win32_Security`, because `windows-sys 0.59` compiles `CreateFileW` only with this feature (its `lpSecurityAttributes` parameter is a `*const SECURITY_ATTRIBUTES`; the binding is `#[cfg(feature = "Win32_Security")]`);
  - `Win32_System_Diagnostics_Debug`, for `SetThreadErrorMode` and `SEM_FAILCRITICALERRORS`;
  - `Win32_System_SystemInformation`, for `GlobalMemoryStatusEx`, `MEMORYSTATUSEX`, and `GetLocalTime`;
  - plus the TUI's existing `Win32_UI_Shell` and `Win32_UI_WindowsAndMessaging`;
  - plus any feature another audit change has added by the time this one lands, for example `Win32_System_Console` (`process-lifecycle-hardening`) and `Win32_System_Com` (`offload-blocking-io`). The list is the union at landing time, not a fixed set.

  The module of each symbol above was checked against the `windows-sys 0.59.0` sources in the cargo registry; it is re-confirmed by compilation while implementing.
- Core adds `[target.'cfg(windows)'.dependencies] windows-sys = { workspace = true }`. Non-Windows builds pull in nothing new.
- The hand-written `extern` blocks, struct layouts (`MemoryStatusEx`, `ByHandleFileInformation`, `FileTime`, `Win32FindStreamData`, `SystemTime`), constants, and the `HANDLE`/`c_void` aliases are deleted in favor of the `windows-sys` definitions. Behavior does not change. The same calls are made with the same arguments: share flags, `FILE_FLAG_BACKUP_SEMANTICS`, the optional `FILE_FLAG_OPEN_REPARSE_POINT`, the 261-`u16` volume-name buffer, and `SEM_FAILCRITICALERRORS`.
- The rule targets *declarations*: `extern "system" { … }` blocks that write out a Windows prototype, and `#[repr(C)]` copies of Windows structures. A Rust *callback* that Windows calls, defined as `extern "system" fn …` (a console control handler from `process-lifecycle-hardening`, a `CopyFileExW` progress routine from `durable-file-copy`), stays allowed. Its signature is checked against the `windows-sys` type it is passed as (`PHANDLER_ROUTINE`, `LPPROGRESS_ROUTINE`).
- Every remaining `unsafe` block gets a `// SAFETY:` comment stating its invariant. The audit's appendix gives the invariants: buffers are sized and their lengths passed, wide strings are NUL-terminated and outlive the call, handles are closed on every path, `dwLength` is preset, and out-pointers are valid.
- `[workspace.lints.clippy] undocumented_unsafe_blocks = "warn"` with `[lints] workspace = true` in both crates. Together with `-D warnings` in CI, a new undocumented `unsafe` block fails the PR. This is the only lint added at workspace level. Broader lint policy is out of scope.

*Alternative considered: the `windows` crate (safe-ish wrappers).* It is heavier, it would be a second Microsoft binding crate next to the TUI's `windows-sys`, and its COM-oriented API buys nothing for these calls. **Rejected.**

### D7: Clean baseline first, then the gates

The gates go live only once they pass:
- Every clippy warning (26 in the audit's count, 29 in the review re-run; see Context) is fixed in place. Each fix is mechanical. `large_enum_variant` on `editor::LoadResult` is fixed by boxing the large variant. `too_many_arguments` may be allowed locally with a reason when a renderer genuinely takes that many inputs. The existing `#[allow(clippy::too_many_arguments)]` in `views/panel.rs` is the precedent.
- The rustdoc warnings are fixed by correcting paths, by turning links to private items into plain code spans (public docs must not point at private items), by escaping `<path>` and `<year>` as code, and by disambiguating `update` (`fn@update` or `mod@update`). `--document-private-items` is not used, so the published docs stay public-API docs.

Commit order on the build branch:
1. the format commit;
2. the baseline fixes;
3. manifests and FFI;
4. the CI workflow;
5. `.git-blame-ignore-revs` with the format commit's hash.

### D8: README corrections

- **"Building from source"** names the minimum Rust version (from D3). It notes that Linux and macOS builds are best-effort and need only a C compiler (for the vendored libgit2 and zlib), not OpenSSL headers.
- **"Testing"** describes what the suites actually cover: core unit and property tests, mostly against the in-memory `FakeFs`; the git-info tests against fixture repositories created in temporary directories; and the TUI snapshot tests. It names the commands CI runs, so a contributor can reproduce CI locally. If `durable-file-copy`'s `RealFs` suite has landed by then, the section says so.

## Risks / Trade-offs

- [The format commit conflicts with every open `build/` branch.] → Land it when few branches are open (proposal: Coordination). It contains no logic, so a branch that conflicts can resolve by re-running `cargo fmt` after rebasing. `.git-blame-ignore-revs` keeps blame useful.
- [A pinned CI toolchain falls behind local stable, so contributors on a newer stable may see lints CI does not.] → Bump it deliberately. Local `cargo clippy` warnings are advisory until the bump.
- [Release builds take longer with thin LTO and one codegen unit.] → Only `--release` is affected. `installer/build.ps1` builds release once per installer build.
- [The `msrv` job adds a second Windows build per PR.] → It is `cargo check` only (no codegen or tests) and cached.
- [`windows-sys` feature names are wrong for a symbol.] → Compilation fails loudly. There is no undefined-behavior risk, which is the point of the move.
- [The Linux job may be red from day one if the `cfg(not(windows))` paths do not compile.] → It is allowed to fail. A red result is useful information, not a blocker.

## Migration Plan

Contributors run `cargo fmt --all` before committing, as CI now checks it. Branches opened before the format commit rebase and re-run `cargo fmt`. The `main` branch-protection rule (required `windows` and `msrv` checks) is enabled by the user after the workflow's first green run on the PR. There are no runtime or data changes.

## Open Questions

None. The `rust-version` value is fixed by the empirical procedure in D3 during implementation.
