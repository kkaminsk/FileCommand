# Change: config-parsing-robustness

## Why

FileCommand reads three hand-written formats from the profile directory: `config.toml` (flat TOML-like lines plus a `[mouse]` table), `usermenu.toml` (a TOML array of tables), and `history.json`. Each has its own parser, and each fails silently in ways the 2026-10-12 code audit confirmed:

- **R-16 (Medium): the README example silently disables every key binding, and inline comments break values.**
  - The README's `config.toml` example lists all `key.*` lines *after* `[mouse]`. The parser correctly scopes them to that table, where only `enabled` is recognized, so every custom binding is dropped. No warning is given.
  - Inline comments are not stripped. `splash = true # comment` fails `parse_bool`, `theme = "x" # c` keeps the quotes and the comment as the theme name, and `panel_split = 40 # c` is not an integer.
  - Design §3.1 promises warnings for unknown keys, but `config::parse` collects none.
- **R-15, config part (Medium): BOMs break the files, and config writes are not crash-durable.**
  - `parse` does not strip a UTF-8 BOM, because U+FEFF is not whitespace. A `config.toml` saved by older Notepad therefore silently ignores its first key.
  - `parse_usermenu` sees `\u{feff}[[entry]]`, treats the whole file as malformed, and falls back to the default menu.
  - `save_theme_atomic` and `save_panel_split_atomic` write a temp file and rename it without `sync_all`, so a power loss can leave a zero-length `config.toml`.
- **B-05 (Low): the bespoke parsers' silent failure modes.**
  - Three dialects with no shared code and no diagnostics.
  - The history JSON reader stops at the first `\u` escape that is not a standalone scalar value (a lone `\uD83D`, and even a valid surrogate pair) and **drops every later command** (`parse_history`).
  - `extract_value_span` finds `"commands"`/`"directories"` by raw text search instead of by structure.

Verifying these against the code turned up related silent failures in the same paths:

- A `config.toml` that is not valid UTF-8 (an ANSI file containing `é`) is discarded completely without a word.
- A `usermenu.toml` that is not valid UTF-8 is treated as *missing*, and `load_user_menu` **overwrites it with the defaults**. That breaks user-menu's "MUST NOT silently overwrite" rule.
- `usermenu.toml`'s non-fatal warnings (unknown keys, entries missing a field) are collected but never shown, although user-menu "Unknown keys warn rather than fail" requires a warning.
- `set_theme_line`/`set_panel_split_line` append a missing key at the *end* of the file. In a file that ends with a `[mouse]` table, the new key lands inside `[mouse]` and is ignored on the next start.

## What Changes

- **New `configuration-files` capability**, describing how the profile files are read and written. Today no spec owns parsing; the requirements that mention these files are scattered across a dozen capabilities, among them theme-selection, panel-split, mouse-input, startup-splash, external-editor, command-line, user-menu, and fuzzy-jump.
- **What "one shared tokenizer" means here.** `config.toml` and `usermenu.toml` are line-oriented TOML-like files, and both are read through one shared line tokenizer (BOM strip, comment and quote rules, table headers, line numbers, warnings). `history.json` is JSON, not line-oriented, so it does **not** go through that tokenizer: it shares only the BOM strip, and its two duplicated string-escape loops are merged into one JSON string decoder. This is how this change applies the audit's "route all three through one tokenizer" (B-05) and the user decision "one shared tokenizer".
- **BOM and inline comments.**
  - A leading UTF-8 BOM is ignored in `config.toml`, `usermenu.toml`, and `history.json`.
  - In `config.toml` and `usermenu.toml`, a `#` outside a quoted string starts a comment that runs to the end of the line, on key lines and table-header lines alike. A `#` inside a quoted value is kept.
  - In `config.toml`'s unquoted (bare) values, a `#` starts a comment only after whitespace, so a bare `C:\Tools\C#\edit.exe` keeps working.
- **Warnings instead of silence.**
  - `config.toml` and `usermenu.toml` are read through one shared line tokenizer that collects warnings with line numbers.
  - `config.toml` warns about:
    - a line that is not `key = value`, a table header, or a comment;
    - an unknown key;
    - a key placed under the wrong table (for example `key.paste_name` after `[mouse]`), naming the line and explaining that top-level settings must come before the first table;
    - an unknown table;
    - a malformed quoted value;
    - a boolean or key-binding value that cannot be read;
    - a file that is not valid UTF-8 (defaults are used).
  - `usermenu.toml`'s existing warnings, and a new "not UTF-8" outcome (treated as malformed, file left untouched), are surfaced too.
  - Warnings are shown once at startup through the existing dismissable startup-warning dialog, as a one-line summary (first warning plus a "+N more" count). Warnings from a user-menu reload after an edit are shown the same way.
  - The summary has the lowest precedence in the single startup-warning field: any other startup warning (an empty `--config-dir`, the profile-directory fallback, an unknown `--theme`, a malformed `usermenu.toml`, the legacy-files notice, an unwritable profile) is shown instead, so no warning that existing requirements mandate is displaced.
  - A not-UTF-8 `usermenu.toml` gets its own message (`usermenu.toml is not UTF-8 text; using the default menu — re-save it as UTF-8 or delete it to recreate the defaults`) instead of the generic malformed one.
  - `theme` and `panel_split` keep their already-specified silent fallbacks.
- **History JSON decoding** (the B-05 escape fix lives here, not in `history-persistence-hardening`):
  - `\uXXXX` surrogate pairs decode to their character;
  - a lone or invalid `\u` escape decodes to U+FFFD, and parsing continues, so no later entry is lost;
  - the `"commands"`/`"directories"` keys are found only at the top level of the JSON object.

  `history.json` problems stay silent, as fuzzy-jump "Missing or malformed history file" requires.
- **Table-aware, durable rewrites.**
  - Theme and panel-split rewrites go through the same tokenizer. They replace only the top-level line, keep its trailing comment and any leading BOM, and insert a missing key *before* the first table header instead of at the end of the file.
  - Every write of `config.toml` and `usermenu.toml` (including the first-run default user menu) goes to a uniquely named temp file that is flushed with `sync_all` before it replaces the target. This uses the shared durable-replace helper.
- The parsers stay hand-rolled, with **no new crates** (user decision).
- The README's `config.toml` example is fixed at implementation time: `[mouse]` moves to the end, and the "Unknown keys are ignored" text becomes "produce a startup warning".

## Capabilities

### New Capabilities

- `configuration-files`:
  - BOM and inline-comment tolerance for the profile files;
  - startup warnings for malformed, unrecognized, misplaced, and non-UTF-8 configuration content;
  - lossless history JSON string decoding;
  - table-aware rewrites;
  - durable writes of `config.toml` and `usermenu.toml`.

### Modified Capabilities

None. No existing requirement is replaced:

- the theme-selection and panel-split fallbacks remain silent;
- user-menu's malformed-file handling and its warn-on-unknown-key rule are what this change now implements faithfully;
- `history.json` write durability is specified by `history-persistence-hardening` in fuzzy-jump.

## Impact

- **Specs:** new `configuration-files` capability (five ADDED requirements).
- **Code (core):**
  - `crates/filecommand-core/src/config.rs`:
    - new shared line tokenizer: BOM strip, comment stripping, quoted and bare values, table headers, line numbers, warnings;
    - `parse` rebuilt on it and returning warnings alongside `Config`;
    - `parse_bool`, `parse_string`, and the `panel_split` integer parse receive comment-free values;
    - `load` distinguishes NotFound from invalid UTF-8;
    - `parse_usermenu` rebuilt on the tokenizer;
    - `load_user_menu` no longer overwrites a non-UTF-8 file, and writes defaults through the durable helper;
    - `set_theme_line`/`set_panel_split_line` become table-aware and keep comments and the BOM;
    - `save_theme_atomic`/`save_panel_split_atomic` use the durable helper;
    - shared `decode_json_string` used by `parse_history` and `extract_string_field`;
    - top-level-only key lookup replaces `extract_value_span`'s raw `find`;
    - BOM strip in `parse_history_file`.
- **Code (TUI):**
  - `crates/filecommand-tui/src/app.rs`:
    - `run` collects config and user-menu warnings into the startup-warning summary, which is stored only when no other startup warning was raised (lowest precedence);
    - `apply_user_menu` surfaces `warnings`, and the not-UTF-8 message.
- **Code (core, reload path):** `apply_user_menu_reload` in `crates/filecommand-core/src/update.rs` surfaces reload warnings. It receives the `UserMenuLoadResult` that the TUI's `app::reload_user_menu` sends back as `Command::UserMenuReloaded`; `reload_user_menu` itself does not change.
- **Docs:** README `config.toml` example and the unknown-keys sentence; module docs in `config.rs`.
- **Tests:**
  - unit tests in `config.rs` for every tokenizer rule, warning, rewrite, and JSON escape case;
  - `app.rs` tests for the warning summary and its precedence;
  - a temp-dir test that a non-UTF-8 `usermenu.toml` is not overwritten.
- **Compatibility:**
  - A value that today contains a ` #` sequence outside quotes (for example `shell = cmd /C echo #1`) now ends at the comment. This is TOML's rule, and the fix is to quote the value.
  - Quoted `config.toml` values keep today's verbatim treatment (backslashes are not escapes), so `editor = "C:\Tools\ed.exe"` keeps working.
  - Files without comments, BOMs, or errors parse exactly as before.

**Audit findings:** R-15 (config.toml and usermenu.toml part), R-16, B-05

**Coordination:**
- `history-persistence-hardening` (P-08, R-13, R-15 history part) changes `history.json`'s format marker, merge, cap, and writer. This change owns only the history *reader's* BOM handling and string-escape decoding. Both edit the history section of `config.rs`; whichever lands second rebases onto the other's helpers.
- `editor-encoding-safety` introduces the shared durable-replace helper (temp file, `sync_all`, replace). This change routes the `config.toml` and `usermenu.toml` writes through it, and introduces it if this change lands first.
- `error-diagnostics` (B-06) adds a diagnostics log. Once it exists, the full list of configuration warnings should also go to the log; the startup dialog shows only the summary. Neither change depends on the other.
- `ci-and-build-tooling` (B-09) also corrects README claims. The `config.toml` example reorder and the unknown-keys sentence belong to this change.
- `config-profile-directory` (in flight; already implemented on the build branch) defines where the three files live and raises the profile-directory startup warnings through the single last-writer-wins `startup_warning` field. This change gives its summary the lowest precedence, so every profile-directory warning still wins.
- `offload-blocking-io` (P-09) covers the synchronous effects on the UI thread, whose audit location includes the theme and panel-split persist writes. This change adds a `sync_all` to those writes but does not move them off the UI thread.
