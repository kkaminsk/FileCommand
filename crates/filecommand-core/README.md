# filecommand-core

Platform-agnostic application logic for FileCommand. No `ratatui` or
`crossterm` dependency — this crate performs no terminal I/O.

## Build requirements

Most of this crate is pure Rust with no native toolchain requirements. One
dependency is the exception:

- **`git2`** (used by the `git_info` capability, M5 §3.1) wraps `libgit2`
  via the `libgit2-sys` crate. Its build script either links a system
  `libgit2` found through `pkg-config`/`vcpkg`, or compiles the vendored
  copy bundled in `libgit2-sys` from source, which requires a C toolchain
  (an MSVC installation on Windows, or `cc`/`make` elsewhere) — no `cmake`
  or `perl` install step is needed beyond that toolchain. If a CI image or
  a contributor's machine lacks a working C compiler, `cargo build`/`cargo
  check` for this crate will fail at the `libgit2-sys` build-script step,
  not at `git2`'s own compilation.

### Fallbacks if `git2`/`libgit2` proves too heavy (design D3)

`git_info`'s dedicated worker-thread design does not depend on `git2`
specifically — only on *some* way to resolve a repository's branch name and
per-file status, off the UI thread. If the native build dependency becomes a
problem (slow CI images, cross-compilation targets without a C toolchain,
etc.), two fallbacks are documented as acceptable substitutes without a
design change:

1. **`gitoxide`** (the `gix` crate) — a pure-Rust git implementation with no
   C build dependency. Heavier to integrate (a different, lower-level API)
   but removes the native toolchain requirement entirely.
2. **`git status --porcelain` subprocess** — shells out to the user's
   installed `git` binary and parses its stable porcelain output. Avoids
   any new library dependency at the cost of requiring `git` on `PATH` and
   paying process-spawn latency per query (acceptable since queries already
   run on a dedicated worker thread and never block the UI).

Either fallback keeps the same worker-thread isolation, pathspec-scoped
queries, and generation-key staleness guard `git_info` is designed around
(M5 design D3) — only the branch/status *resolution* mechanism changes.
