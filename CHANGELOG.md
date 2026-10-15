# Changelog

All notable changes to FileCommand are listed here, newest first. Each
release is on the [GitHub releases page](https://github.com/kkaminsk/FileCommand/releases)
with its signed installer.

## [Unreleased]

### Changed

- The winget manifest template's short description no longer names a
  third-party product, matching the wording winget approved. Manifests
  generated from the template now match what was published (#57).
- `installer\build.ps1 -Sign` looks for the signing tooling in a
  `CodeSigning` checkout next to the repository, instead of
  `C:\GitHub\CodeSigning` (#57).
- Added this changelog; the README now covers installing with winget and
  Chocolatey, safer copies, and junction listings (#57).

## [0.1.5] - 2026-10-15

A large reliability and usability release: every finding from the October
2026 code audit is fixed, plus nine usability improvements (#55, #56).

### Fixed

- Copying a file onto itself no longer truncates it to 0 bytes.
- A cross-volume Move no longer deletes files inside junction or symlink
  targets. Deleting a junction now works on Windows.
- A failed or cancelled copy never leaves a truncated file, and a failed
  Overwrite keeps the original: copies are written to a temporary partial
  file and renamed into place only once complete.
- Moves flush and verify each file before deleting the source, and a
  source that can't be removed is reported instead of ignored.
- Copies keep attributes, creation and modification times, and alternate
  data streams.
- The built-in editor sends non-UTF-8, UTF-16, and binary files to the
  viewer instead of corrupting them on save, keeps a UTF-8 BOM, and saves
  atomically. Save failures are now shown.
- Shrinking the terminal below 60×16 no longer discards an unsaved editor
  buffer, the viewer, a running copy, or an open dialog.
- Relative F5/F6 destinations and `cd`/`del`/`rmdir` targets resolve against
  the panel's directory, not the directory FileCommand was started in.
- A listing the user has already moved away from can no longer leak
  entries into the current panel or tab.
- The quick filter never lets a command act on an entry it hides, discloses
  hidden selections in the delete and copy/move dialogs, and matches
  non-ASCII names correctly.
- Ctrl+C stops only a running child command, not FileCommand.
- A crash in a background worker no longer garbles the screen or leaves a
  panel stuck at `Reading…`.
- The viewer no longer locks the file it shows, so programs can truncate or
  rotate a log while it is open.
- Git status no longer piles up slow queries in a huge or remote repository,
  and works in directories whose names contain `[`.

### Added

- Settings live in a per-user profile directory (`%APPDATA%\FileCommand`),
  with a `--config-dir` flag and a portable mode.
- Caret editing, Home/End, word movement and deletion, and paste in every
  text field and the command line. Ctrl+Y clears the command line.
- An Open entry in the file-action menu launches a file in its associated
  application.
- Commands → History (Alt+F8), Swap panels (Ctrl+U), and Menu file edit;
  greyed-out pull-down items are now wired up or removed.
- Cancel (Esc or C) in the overwrite-conflict dialog. Quitting during a copy
  waits for its cleanup.
- Animated splash, menus, and dialogs, with `--noanimations` and
  `animations = false` to turn them off.
- An optional diagnostic log via `--log <path>` or `FILECOMMAND_LOG`, and
  error messages that name the file or folder involved.
- The installer warns when FileCommand is already installed in the other
  scope.
- Continuous integration on every pull request.

### Changed

- Large directories stream in with a live count and stay responsive.
- Opening, viewing, saving, and checking paths on slow or offline network
  shares no longer freezes the screen.
- No redraws while idle; key repeats and pastes apply in one burst; copy
  progress updates are throttled.
- Junctions and directory symlinks list as enterable directories, and an
  unreadable entry no longer fails the whole listing.
- Full mode shows Size, Date, and Time untruncated. Hidden and system
  entries render in a dimmer colour.
- Re-reading a directory keeps the cursor on the same entry.
- `config.toml` and `usermenu.toml` tolerate a BOM and inline comments and
  report mistakes at startup. `history.json` keeps recency across restarts,
  is capped, and merges safely between two running instances.

## [0.1.4] - 2026-10-05

### Fixed

- The `del` and `rmdir` command-line builtins reject `.` and `..` targets;
  previously `rmdir ..` opened the delete confirmation on the panel's parent
  directory (#46).
- Switching to a stale background tab no longer records a fuzzy-jump visit
  or rewrites `config.toml` (#47).

### Added

- A Chocolatey package channel, and README guidance on reporting issues.

## [0.1.3] - 2026-09-27

### Added

- The Delete key opens the delete confirmation, the same as F8.

## [0.1.2] - 2026-09-26

### Added

- `-h`, `-?`, and `--help` print usage and exit without starting the
  interface.

## [0.1.1] - 2026-09-25

### Fixed

- The file-action menu responds to mouse clicks.
- The inactive panel refreshes after a delete when it shows the same
  directory.

### Changed

- The command line recognizes its built-in verbs directly instead of
  starting a shell.
- The file-action menu lists the clipboard entries earlier.

## [0.1.0] - 2026-09-03

First release: a keyboard-driven, dual-panel terminal file manager.

- Dual panels with Full, Brief, and Tree modes, an adjustable split, tabs,
  quick filter, type-ahead, and fuzzy directory jump.
- Copy, move, rename, and delete with confirmations, multi-selection,
  clipboard export, and error recovery.
- Command line, pull-down menus, drive selector, and a file-action menu.
- Built-in viewer and editor, plus an external editor option.
- Mouse support, including drag and drop between panels.
- Switchable themes, git status in the panels, find-file, and a startup
  splash.

[Unreleased]: https://github.com/kkaminsk/FileCommand/compare/v0.1.5...HEAD
[0.1.5]: https://github.com/kkaminsk/FileCommand/releases/tag/v0.1.5
[0.1.4]: https://github.com/kkaminsk/FileCommand/releases/tag/v0.1.4
[0.1.3]: https://github.com/kkaminsk/FileCommand/releases/tag/v0.1.3
[0.1.2]: https://github.com/kkaminsk/FileCommand/releases/tag/v0.1.2
[0.1.1]: https://github.com/kkaminsk/FileCommand/releases/tag/v0.1.1
[0.1.0]: https://github.com/kkaminsk/FileCommand/releases/tag/v0.1.0
