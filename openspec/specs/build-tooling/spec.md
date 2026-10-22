# build-tooling Specification

## Purpose
TBD - created by archiving change ci-and-build-tooling. Update Purpose after archive.
## Requirements
### Requirement: Continuous integration gates every pull request

The repository SHALL contain a GitHub Actions workflow that runs on every pull request and on every push to `main`. A required job on `windows-latest` MUST run, against the committed `Cargo.lock`:
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS=-D warnings`

The job MUST fail if any of these steps fails. A job on `ubuntu-latest` SHALL build and test the workspace on a best-effort basis and MUST be marked allowed-to-fail, so that its result is reported but never blocks a merge. The toolchain used by the required Windows job SHALL be pinned to a specific stable release in the workflow and changed only deliberately. The README SHALL list the commands the required job runs, so a contributor can reproduce it locally.

#### Scenario: A formatting slip fails the pull request

- **WHEN** a pull request changes a `.rs` file so that `cargo fmt --all -- --check` reports a diff
- **THEN** the required Windows job fails and the pull request shows a failing check

#### Scenario: A new clippy warning fails the pull request

- **WHEN** a pull request introduces code that produces any clippy warning on any target, tests included
- **THEN** the required Windows job fails

#### Scenario: A failing test fails the pull request

- **WHEN** any test in the workspace fails on `windows-latest`
- **THEN** the required Windows job fails

#### Scenario: A broken doc link fails the pull request

- **WHEN** a doc comment links to an item that does not exist or is private
- **THEN** `cargo doc` with `-D warnings` fails and the required Windows job fails

#### Scenario: Linux results never block a merge

- **WHEN** the `ubuntu-latest` job fails to build or test
- **THEN** its failure is visible on the pull request
- **AND** the pull request's required checks are unaffected

#### Scenario: Clean baseline

- **WHEN** the workflow first runs on the branch that introduces it
- **THEN** every step of the required Windows job passes, with no formatting diffs, clippy warnings, test failures, or rustdoc warnings

### Requirement: Formatting configuration is committed

The repository SHALL commit a `rustfmt.toml` at its root that reproduces the existing source style as closely as stable rustfmt options allow: `max_width = 150` and `use_small_heuristics = "Max"`. The whole workspace SHALL be formatted with it once, in a commit that contains only formatting changes, and that commit's hash SHALL be listed in a committed `.git-blame-ignore-revs` file. After that commit, `cargo fmt --all -- --check` MUST report no differences on `main`.

#### Scenario: Format check is clean on main

- **WHEN** `cargo fmt --all -- --check` runs on a fresh checkout of `main`
- **THEN** it reports no differences and exits successfully

#### Scenario: The reformat commit is isolated

- **WHEN** the reformat commit is inspected
- **THEN** it changes only whitespace, line breaks, and other rustfmt output, with no change to any token's meaning
- **AND** `git blame` configured with `.git-blame-ignore-revs` attributes lines to their pre-reformat authors

#### Scenario: CRLF working trees pass

- **WHEN** the repository is checked out on Windows with `core.autocrlf = true` and `cargo fmt --all -- --check` runs
- **THEN** it reports no differences caused by line endings

### Requirement: Toolchain floor and release profile are declared

The workspace SHALL declare its minimum supported Rust version as `rust-version` in `[workspace.package]`, inherited by both crates. The declared value MUST be a toolchain on which the workspace, including all targets, builds against the committed `Cargo.lock`, and CI MUST check this on every pull request. The workspace SHALL define `[profile.release]` with `lto = "thin"`, `codegen-units = 1`, and `strip = "symbols"`. The release profile MUST keep the `unwind` panic strategy, because terminal restoration and worker-thread panic isolation depend on unwinding. The README's build instructions SHALL name the minimum Rust version.

#### Scenario: Older toolchain is rejected clearly

- **WHEN** a contributor builds with a toolchain older than the declared `rust-version`
- **THEN** cargo stops with its standard message naming the required version, instead of failing with a compile error deep in the code

#### Scenario: The floor is verified

- **WHEN** a pull request uses a standard-library API newer than the declared `rust-version`
- **THEN** the CI job that checks the workspace on the declared toolchain fails

#### Scenario: Release binary is optimized and stripped

- **WHEN** `cargo build --release` produces `filecommand.exe`
- **THEN** it is built with thin LTO, a single codegen unit, and `strip = "symbols"`
- **AND** on targets whose binaries embed a symbol table (ELF, Mach-O), the release binary carries none

#### Scenario: Panics still unwind in release builds

- **WHEN** a release build panics on the main thread while the terminal is in raw mode on the alternate screen
- **THEN** the terminal is restored before the panic report is printed, exactly as in a debug build

### Requirement: libgit2 is built with only the features local status needs

The `git2` dependency SHALL be declared with `default-features = false`, enabling no network transport (SSH or HTTPS), because git-info reads only local repository state. The Linux build MUST NOT require OpenSSL development headers. Git-info behavior SHALL be unchanged: branch name and per-entry status on Windows and elsewhere.

#### Scenario: No SSH or OpenSSL in the dependency graph

- **WHEN** the resolved dependency graph of `filecommand-core` is inspected (for example with `cargo tree -p filecommand-core`)
- **THEN** `libssh2-sys`, `openssl-sys`, and `openssl-probe` do not appear in it

#### Scenario: Linux builds without OpenSSL headers

- **WHEN** the workspace is built on an Ubuntu machine with a C compiler but without the OpenSSL development package
- **THEN** the build does not fail for lack of OpenSSL

#### Scenario: Git info still works

- **WHEN** a panel shows a directory inside a git working tree with a modified, a staged, and an untracked file
- **THEN** the branch name and the three status markers are reported exactly as before

### Requirement: Windows FFI uses windows-sys and every unsafe block documents its invariant

All Windows API calls in both crates SHALL use the prototypes, constants, and struct definitions from the `windows-sys` crate, declared as a Windows-only dependency. The workspace MUST NOT declare its own Windows prototypes (`extern "system" { … }` declaration blocks) or `#[repr(C)]` copies of Windows structures. Rust callback functions that Windows invokes, defined with the `extern "system"` ABI (for example a console control handler or a copy-progress routine), are permitted; their signatures SHALL match the corresponding `windows-sys` function-pointer type. Every `unsafe` block in the workspace MUST be immediately preceded by a `// SAFETY:` comment that states the invariant making it sound, and the workspace SHALL enable `clippy::undocumented_unsafe_blocks` so that CI rejects an undocumented one.

#### Scenario: No hand-declared Windows prototypes

- **WHEN** the workspace sources are searched for `extern "system" { … }` declaration blocks or `#[repr(C)]` structs mirroring Windows types
- **THEN** none are found, and drive enumeration, volume information, free space, memory status, file identity, alternate-data-stream enumeration, and local time all call through `windows-sys`
- **AND** any `extern "system" fn` found is a callback definition passed to a Windows API, not a prototype

#### Scenario: Non-Windows builds do not pull in windows-sys

- **WHEN** the workspace is built for a non-Windows target
- **THEN** `windows-sys` is not compiled as a dependency of either crate

#### Scenario: Undocumented unsafe fails CI

- **WHEN** a pull request adds an `unsafe` block without a preceding `// SAFETY:` comment
- **THEN** clippy reports `undocumented_unsafe_blocks` and the required Windows job fails

#### Scenario: Behavior is unchanged

- **WHEN** the drive-select dialog, the Info panel, a copy that preserves alternate data streams, and a case-only rename run on Windows after the migration
- **THEN** they produce the same results as before it

