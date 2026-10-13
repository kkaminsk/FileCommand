# Tasks: config-parsing-robustness

## 1. Shared tokenizer (`crates/filecommand-core/src/config.rs`)

- [ ] 1.1 Add `strip_bom(&str) -> &str`, which removes one leading U+FEFF (configuration-files "Configuration files tolerate a UTF-8 BOM and inline comments"; design D1).
- [ ] 1.2 Add `tokenize(input, QuoteMode) -> Vec<Line>`:
  - line numbers are 1-based;
  - `Line` is `Blank`, `Table { name, array }`, `KeyValue { key, value: Basic | Literal | Bare, comment_span }`, or `Malformed { reason }`;
  - the closing quote is the earliest one leaving only whitespace or a `#` comment;
  - a `#` in a bare value starts a comment only at the value start or after whitespace;
  - table headers may carry a trailing comment;
  - `QuoteMode::Verbatim` for `config.toml`, `QuoteMode::TomlEscapes` for `usermenu.toml`, so that `\"` does not close a basic string (design D1).
- [ ] 1.3 Add `ConfigWarning { file, line, message }` and a summary formatter: the first warning plus ` (+N more)` (design D2, D4).

## 2. `config.toml` parsing and loading

- [ ] 2.1 Rebuild `parse` on `strip_bom` and `tokenize`, returning `ParsedConfig { config, warnings }`. Keep every existing key, default, `[mouse]` scoping rule, and the verbatim quoted-value semantics of `parse_string` (configuration-files "Hash inside a quoted value is kept"; design D1).
- [ ] 2.2 Emit the warnings in design D2's table:
  - malformed line;
  - unterminated quote;
  - unknown key;
  - a key under `[mouse]` other than `enabled`, with the "top-level settings must come before the first [table]" wording;
  - unknown table (one warning per header);
  - an uninterpretable boolean or `key.*` value.

  Emit no warning for the value of a well-formed `theme` or `panel_split` line; an unterminated quote on such a line is still reported as malformed (configuration-files "Malformed or unrecognized configuration lines produce startup warnings", "Unterminated quote is reported").
- [ ] 2.3 Make `load` return `ParsedConfig`:
  - `NotFound` → defaults, no warning;
  - invalid UTF-8 → defaults plus `config.toml is not UTF-8 text; using default settings`;
  - other read errors → defaults plus a `could not be read` warning with the OS message.
- [ ] 2.4 Update the callers of `parse`/`load` (`app::run`, tests) for the new return type.

## 3. `usermenu.toml`

- [ ] 3.1 Rebuild `parse_usermenu` on `strip_bom` and `tokenize(…, TomlEscapes)`, keeping today's outcomes:
  - a `Bare` value, a `Malformed` line, or a table other than `[[entry]]` is malformed;
  - an unknown key, a key outside an entry, or an entry missing a field is a warning, now carrying line numbers.

  BOMs, trailing comments, and `[[entry]] # c` are accepted (configuration-files "BOM-prefixed usermenu.toml loads its entries", "Inline comment in usermenu.toml"; design D3).
- [ ] 3.2 `load_user_menu`: write the default file only on `ErrorKind::NotFound`. Content that is not valid UTF-8 (including UTF-16), or any other read error, returns `malformed: true` with default entries and leaves the file untouched. Add a field to `UserMenuLoadResult` that carries the reason, so `apply_user_menu` shows the not-UTF-8 message from design D3 instead of the generic malformed one (configuration-files "Non-UTF-8 usermenu.toml is kept, not overwritten"; design D3).

## 4. Surfacing warnings

- [ ] 4.1 `crates/filecommand-tui/src/app.rs` `run`: collect the `config.toml` warnings, then the `usermenu.toml` warnings, and give the summary the lowest precedence: once both files are loaded and every other startup warning (`--config-dir`, profile-resolve, `--theme`, user-menu-malformed, legacy, unwritable-profile) has been assigned, set the summary into `state.startup_warning` only if it is still `None` (configuration-files "Several warnings are summarized", "Profile warning keeps precedence", "Theme override warning keeps precedence"; design D4).
- [ ] 4.2 `apply_user_menu` (startup): surface `warnings` as part of the summary in 4.1. `malformed` keeps its existing message and precedence.
- [ ] 4.3 `crates/filecommand-core/src/update.rs` `apply_user_menu_reload`: when the result is not malformed but has warnings, set `startup_warning` to their summary and apply the entries (configuration-files "Warnings after editing the user menu"; design D4).
- [ ] 4.4 Update the module docs in `config.rs`: comment and BOM rules, warnings, and the removal of "unrecognized keys are ignored".

## 5. Table-aware, durable rewrites

- [ ] 5.1 Add `set_top_level_key(input, key, rendered)` on `tokenize`:
  - replace the first occurrence of the key before the first table header, keeping its trailing comment;
  - otherwise insert before the first header, or append when there is none;
  - re-emit a leading BOM;
  - leave other lines byte-identical.

  Re-implement `set_theme_line`/`set_panel_split_line` as wrappers (configuration-files "Configuration rewrites keep settings in the top-level table"; design D5).
- [ ] 5.2 Route `save_theme_atomic`, `save_panel_split_atomic`, and `load_user_menu`'s default-file write through the shared durable-replace helper (unique temp name, `sync_all`, replace, cleanup on failure). Introduce the helper if `editor-encoding-safety` has not landed. Delete `temp_sibling` once it is unused, coordinating with `history-persistence-hardening` (configuration-files "Persisted configuration writes are durable"; design D6).

## 6. History JSON reader (B-05)

- [ ] 6.1 Add a shared `decode_json_string`:
  - surrogate pairs combine into one character;
  - a lone surrogate or a bad `\u` becomes U+FFFD, and decoding continues;
  - only an unterminated string fails.

  Use it in `parse_history` and `extract_string_field`, so no later entry is dropped (configuration-files "History file strings decode without losing entries"; design D7).
- [ ] 6.2 Make the key lookup in `extract_value_span` scan only top-level object keys, outside string literals (configuration-files "Key text inside a string is not mistaken for a key"; design D7).
- [ ] 6.3 Apply `strip_bom` in `parse_history_file` and `parse_history` (configuration-files "BOM-prefixed history.json loads").

## 7. Tests (`config.rs` unit tests unless noted)

- [ ] 7.1 Tokenizer:
  - BOM strip;
  - whole-line comment;
  - `[mouse] # c` header;
  - `[[entry]]` array header;
  - `k = "a # b"` keeps the `#`;
  - `k = "x" # c "d"` → `x`;
  - `k = "C:\dir\"` terminates;
  - `k = C:\Tools\C#\edit.exe` keeps `C#`;
  - `k = v # c` → `v`;
  - `k = "x` → Malformed;
  - `k "x"` (no `=`) → Malformed;
  - line numbers correct with CRLF input.
- [ ] 7.2 `parse` with every recognized key, followed by an inline comment, yields the same `Config` as without comments (R-16 regression: `splash = true # c`, `theme = "x" # c`, `panel_split = 40 # c`).
- [ ] 7.3 R-16 regression using the README example verbatim (`[mouse]` before the `key.*` lines):
  - the bindings stay at their defaults;
  - one warning per misplaced `key.*` line, each naming its line number and "not a [mouse] setting";
  - the corrected example (`[mouse]` last) parses with zero warnings and all ten bindings applied.
- [ ] 7.4 Warnings for:
  - an unknown key;
  - a malformed line;
  - an unknown table (exactly one warning for the header);
  - `animations = yes`;
  - `key.fuzzy_jump = ""`.

  No warning for `theme = "no-such-theme"` or `panel_split = "wide"`; one malformed-line warning for `theme = "purple-lights` (no closing quote). Zero warnings for the default `Config` rendered as a file.
- [ ] 7.5 `load` (temp dir): a missing file gives defaults and no warning; a Windows-1252 file gives defaults and the not-UTF-8 warning; a BOM file parses its first key.
- [ ] 7.6 `parse_usermenu`:
  - BOM plus `[[entry]]` loads;
  - `label = "Build" # c` loads;
  - `[[entry]] # c` loads;
  - a bare value is still malformed;
  - `[foo]` is still malformed;
  - unknown-key and outside-entry warnings carry line numbers.
- [ ] 7.7 `load_user_menu` (temp dir): a UTF-16LE file and a Windows-1252 file each return `malformed: true` with defaults, and the bytes on disk are unchanged afterwards (configuration-files "Non-UTF-8 usermenu.toml is kept, not overwritten").
- [ ] 7.8 `set_top_level_key`:
  - insert above `[mouse]`;
  - replace keeping the trailing comment;
  - BOM preserved with exactly one key line;
  - a `theme` key inside a table is not replaced;
  - no table → append;
  - all other lines byte-identical.

  Then a round trip: re-`parse` the output, get the new value, and no warnings.
- [ ] 7.9 Durable writes: with the helper's fault-injection seam, a failed theme write leaves `config.toml` byte-identical and no temp file; a successful write leaves no temp file.
- [ ] 7.10 History decoding:
  - `\uD83D\uDE00` → 😀;
  - `["cd a","cd b\uD83D","cd c"]` → three commands, the second ending in U+FFFD;
  - `\uZZZZ` → U+FFFD and parsing continues;
  - a stray `\uDE00` → U+FFFD;
  - the string `"directories"` inside the commands array before a `"cd C:\\"` entry does not hide the real `"directories"` key;
  - a BOM-prefixed file parses.
- [ ] 7.11 `crates/filecommand-tui/src/app.rs` tests:
  - the summary shows the first `config.toml` warning plus `(+N more)` across both files;
  - with an unwritable-profile warning present, that warning wins;
  - with a `--theme` warning or a malformed `usermenu.toml` present, that warning wins;
  - a not-UTF-8 `usermenu.toml` shows the not-UTF-8 message;
  - with no warnings, `startup_warning` stays `None` (apart from other sources).
- [ ] 7.12 `update/tests.rs`: `UserMenuReloaded` with warnings and no malformed flag applies the entries and sets the warning summary; a reload that is malformed keeps today's message and keeps the previous entries.

## 8. Docs

- [ ] 8.1 README `config.toml` section:
  - move `[mouse]` / `enabled = true` to the end of the example;
  - state that top-level settings must precede any `[table]` and that `#` starts a comment (quote values containing ` #`);
  - replace "Unknown keys are ignored rather than rejected, and the schema tolerates missing/malformed lines" with a description of the startup warning.
- [ ] 8.2 README `usermenu.toml` section: mention that comments and a UTF-8 BOM are accepted, and that a non-UTF-8 file is reported and never overwritten.

## 9. Verify

- [ ] 9.1 `cargo test --workspace` green.
- [ ] 9.2 `cargo clippy --workspace --all-targets` with no new warnings.
- [ ] 9.3 `openspec validate config-parsing-robustness --strict` clean.
- [ ] 9.4 Manual check:
  - paste the old README example into `config.toml` and start; the warning names the first misplaced `key.*` line with `(+9 more)`;
  - save `config.toml` and `usermenu.toml` as "UTF-8 with BOM" in Notepad and start; both load with no warning;
  - apply a theme to a config that ends with `[mouse]`, restart, and confirm the theme sticks.
