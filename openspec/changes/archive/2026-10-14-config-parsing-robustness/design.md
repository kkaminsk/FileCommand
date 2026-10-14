# Design: config-parsing-robustness

## Context

This was verified against `build/open-proposals-2026-10` at `1bada6f`. All code is in `crates/filecommand-core/src/config.rs` unless noted.

### `config.toml` (`parse`, `load`)

`parse` handles each line as follows:

- It walks `input.lines()` and `trim()`s each line.
- It skips blank lines and lines that *start* with `#`.
- It treats `[...]` as a table header and sets `in_mouse_table` only for `[mouse]`.
- Otherwise it splits on the first `=`.
- Under `[mouse]`, only `enabled` is read; every other key is dropped silently. That is why the README example (`README.md` lines 152–164: `[mouse]` / `enabled = true` / then all ten `key.*` lines) loses every binding.
- Unknown keys fall into `_ => {}`.

Values are read without any comment handling:

- `parse_bool` matches exactly `"true"`/`"false"`.
- `parse_string` strips one pair of matching outer quotes and returns the inner text **verbatim**. Backslashes are not escapes, so `editor = "C:\Tools\ed.exe"` works today.
- `panel_split` uses `value.parse::<u16>()`.

Beyond that:

- A trailing `# comment` makes booleans and integers fail, and corrupts strings.
- A leading U+FEFF survives `trim()` (it is not `White_Space`), so the first line's key becomes `"\u{feff}splash"`.
- `load` uses `std::fs::read_to_string(path)` and maps **every** error, including invalid UTF-8, to `Config::default()`, with no signal.
- No warnings are collected anywhere.

### `usermenu.toml` (`parse_usermenu`, `load_user_menu`)

`parse_usermenu` uses the same line loop:

- `[[entry]]` must match the trimmed line exactly.
- A `key = value` line whose value is not a full `"..."`/`'...'` string (via `parse_toml_string_strict`, which requires the value to *end* with the quote) sets `malformed`. A trailing comment therefore makes the whole file malformed.
- `\u{feff}[[entry]]` is malformed too.
- It already collects `warnings` for unknown keys, keys outside an `[[entry]]` block, and entries missing a field. `load_user_menu` returns them only for well-formed files.
- `app::apply_user_menu` ignores `warnings` and only raises the startup warning for `malformed`. `update::apply_user_menu_reload` does the same on reload. The warnings are never shown.

`load_user_menu` treats **any** read error as "missing": it writes the default menu to the path with `std::fs::write` and returns `created_default`. A `usermenu.toml` saved in ANSI or as UTF-16 ("Unicode" in Notepad) fails `read_to_string` and is overwritten.

### `history.json` reader

- `parse_history_file` locates `"commands"` and `"directories"` with `extract_value_span`, which does a raw `input.find("\"key\"")` and then a bracket-balance scan that is aware of string literals.
- `parse_history`, which reads the command array, decodes `\uXXXX` with `char::from_u32`. That returns `None` for any surrogate, so a lone `\uD83D`, or even the valid pair `\uD83D\uDE00`, hits `None => return out` and **every later command is dropped**.
- `extract_string_field`, which reads a directory path, uses `?` on the same decode, so the one directory entry is skipped.
- The writer, `json_escape`, only emits `\u` for code points below 0x20, so only hand-edited or foreign files hit this.
- There is no BOM handling.

### Rewrites (`set_theme_line`, `set_panel_split_line` → `save_*_atomic`)

The rewrite helpers work line by line:

- They replace the first non-comment line whose key is `theme`/`panel_split`, ignoring which table it is in.
- If there is none, they **append** the key at the end of the file. When the file ends with a `[mouse]` table, the appended key is scoped to `[mouse]` and ignored on the next load.
- A replaced line loses its trailing comment.
- A BOM-prefixed first line is not recognized, so a duplicate key gets appended.

The `save_*_atomic` functions call `std::fs::write` on the fixed `config.toml.tmp`, then `rename`. There is no `sync_all`.

### Startup warnings

`State::startup_warning: Option<String>` is one message. `views/startup_warning.rs` renders it on **one row**, truncated with `…`. `app::run` assigns it last-writer-wins in this order:

1. `--config-dir` flag warning
2. profile-resolve warning
3. `--theme` warning
4. user-menu malformed
5. legacy-files notice
6. unwritable-profile warning

## Goals / Non-Goals

**Goals:**

- A BOM, an inline comment, or a misplaced key never silently changes what a configuration file means.
- Every configuration line FileCommand ignores is reported once, with its file and line number.
- One tokenizer for both TOML-like files, so comment, quote, header, and BOM rules cannot drift between them. `history.json` is JSON and is not line-tokenized; it shares only the BOM strip, and its duplicated string decoders become one (D7).
- The history reader never loses entries to an escape it does not understand.
- FileCommand's own rewrites produce files that read back the way they were intended, and they survive a power loss.
- No new crates.

**Non-Goals:**

- Full TOML or JSON conformance: multi-line strings, arrays and inline tables in `config.toml`, numbers other than `panel_split`, and so on.
- Warnings for `theme` names that are not built-in, or for an invalid `panel_split`. theme-selection "Unknown configured theme falls back to default" and panel-split "Invalid configured value falls back" specify a silent fallback, and this change leaves those requirements unchanged.
- Warnings for `history.json`. fuzzy-jump "Missing or malformed history file" requires a normal launch without error, and the file is machine-written.
- `history.json` format marker, cap, merge, and writer (`history-persistence-hardening`). A logging sink for warnings (`error-diagnostics`).
- Reading non-UTF-8 (ANSI or UTF-16) configuration files. They are reported, not transcoded.
- Duplicate-key detection. The last occurrence still wins, as today.
- Moving the theme and panel-split writes off the UI thread. The audit lists these persist writes under P-09 (`offload-blocking-io`); this change only makes them durable.

## Decisions

### D1: One line tokenizer for `config.toml` and `usermenu.toml`

**Scope of "one shared tokenizer".** The audit (B-05) recommends routing all three files through one tokenizer, and the user decision asks for warnings collected "via one shared tokenizer". The two TOML-like files share the line tokenizer below. `history.json` does not: it is a JSON document, not a sequence of `key = value` lines, and it produces no warnings (Non-Goals). What it shares with the other two is `strip_bom`, and its two copies of the string-escape loop are merged into one `decode_json_string` (D7).

A new private module, `config::toml_lines` (or a section of `config.rs`), provides `tokenize(input: &str) -> Vec<Line>`. The input has already had its leading U+FEFF removed by `strip_bom`. Each `Line` carries its 1-based number and one of:

- `Blank`, which also covers a whole-line comment.
- `Table { name, array: bool }`, for `[name]` or `[[name]]`, with an optional trailing comment.
- `KeyValue { key, value: Value, comment_span }`, where `Value` is one of:
  - `Basic(String)`, the raw inner text of `"…"`;
  - `Literal(String)`, the inner text of `'…'`;
  - `Bare(String)`, unquoted text with surrounding whitespace trimmed.

  The span of the trailing comment is kept, for the rewriter (D5).
- `Malformed { reason }`: no `=`, an empty key, an unterminated quote, or text after a closing quote that is neither whitespace nor a comment.

Comment rules follow TOML where they can, and stay compatible with the bare values `config.toml` already accepts:

- **Quoted values.** The closing quote is the **first** matching quote character such that the rest of the line is empty, whitespace, or whitespace followed by `#…`. With that rule:
  - `theme = "x" # it's "great"` is `x`;
  - `editor = "a # b"` keeps the `#`;
  - `shell = "C:\dir\"` (a trailing backslash) still terminates;
  - `editor = "say \"hi\""` yields its verbatim inner text, as today.

  For `usermenu.toml`, the scan honors TOML basic-string escapes (`\"` does not close the string), because that file is real TOML and its values are unescaped by the existing `unescape_toml_basic`.
- **Bare values.** A `#` starts a comment only at the start of the value or after whitespace. `splash = true # no` is `true`; `editor = C:\Tools\C#\edit.exe` keeps `C#`.
- **Table headers** may carry a trailing comment: `[mouse] # pointer` is the `mouse` table.

The tokenizer does not interpret values. `config.toml` keeps reading `Basic`/`Literal` content verbatim through `parse_string`'s existing semantics. Unescaping it would break `editor = "C:\Tools\ed.exe"`, which works today. `usermenu.toml` keeps applying `unescape_toml_basic` to `Basic` and reading `Literal` raw, and it still rejects `Bare` as malformed. That preserves user-menu's "real TOML" strictness.

Alternatives considered:

- **The `toml` crate.** The audit's alternative was rejected by user decision: no new crates.
- **Comment stripping as a pre-pass with a regex-like split on ` #`.** This cannot tell a quoted `#` from a comment.
- **TOML's strict rule** (any `#` outside a string starts a comment, even inside a bare value). Bare values are already a non-TOML extension, and the whitespace rule protects paths such as `C#`.

### D2: `config.toml` warnings, by category

`parse(input) -> ParsedConfig { config: Config, warnings: Vec<ConfigWarning> }`, where `ConfigWarning { file: &'static str, line: usize, message: String }`. `load(path)` returns the same type. The warnings:

| Condition | Message (example) |
|---|---|
| `Malformed` line | `config.toml line 7: expected key = value; line ignored` |
| Unknown top-level key | `config.toml line 9: unknown setting "colour"; ignored` |
| Key under `[mouse]` other than `enabled` | `config.toml line 12: "key.paste_name" is not a [mouse] setting and was ignored — put top-level settings before the first [table]` |
| Unknown table header | `config.toml line 20: unknown table [colors]; its settings are ignored` (one warning for the header; keys under it are not warned about individually) |
| Boolean key (`splash`, `animations`, `[mouse] enabled`) with a value other than `true`/`false` | `config.toml line 3: splash must be true or false; using the default` |
| `key.*` value that `parse_binding` rejects, or an empty value | `config.toml line 15: key.fuzzy_jump "" is not a key binding; using the default` |
| File exists but is not valid UTF-8 | `config.toml is not UTF-8 text; using default settings` (line 0, not tied to a line) |

`theme` and `panel_split` values are never warned about (see Non-Goals). A missing `config.toml` yields no warning. `load` therefore distinguishes `ErrorKind::NotFound` from other read errors, and other unreadable cases, such as access denied, get a generic `could not be read` warning with the OS message.

### D3: `usermenu.toml` on the tokenizer, never overwritten, warnings surfaced

`parse_usermenu` is rebuilt on `tokenize`. It keeps its current outcomes, but now a BOM, a trailing comment, and `[[entry]] # c` are all accepted:

- An entry key that is not `label` or `command` produces a warning.
- A key outside `[[entry]]` produces a warning.
- An entry missing a field produces a warning, and the entry is skipped.
- A `Malformed`, `Bare`, or non-`[[entry]]` table line makes the file malformed. This matches the current strictness; a `[foo]` header is malformed today, because it is not `[[entry]]` and has no `=`.

The existing warning strings gain line numbers, for example `usermenu.toml line 4: unknown key "icon"; ignored`.

`load_user_menu` creates the default file only for `ErrorKind::NotFound`. Content that is not valid UTF-8 is treated as **malformed**: default entries in memory, the startup warning raised, and the file left untouched. `UserMenuLoadResult` gains a field that says why the file was malformed, so `app::apply_user_menu` shows `usermenu.toml is not UTF-8 text; using the default menu — re-save it as UTF-8 or delete it to recreate the defaults` for this case and keeps today's `usermenu.toml is malformed; F2 uses the default user menu` for invalid TOML. Any other read error (for example access denied) is also treated as malformed, with the OS message, and never overwrites the file. The first-run default write goes through the durable helper (D6).

### D4: Surfacing warnings through the single startup-warning field

`app::run` collects `config.toml` warnings, then `usermenu.toml` warnings, in line order, into one list. It formats one summary line:

- a single warning: `<first message>`;
- more than one: `<first message> (+N more)`.

The dialog is one row wide (`views/startup_warning.rs`), so the first message carries the file and line number the user needs to find the problem. The summary has the **lowest** precedence. In the existing last-writer-wins order it behaves as if it were assigned first; in practice `run` computes it after both files are loaded and stores it only when no other startup warning has been set:

1. **configuration warnings** (new)
2. `--config-dir` flag warning
3. profile-resolve warning
4. `--theme` warning
5. user-menu malformed (including the new not-UTF-8 message)
6. legacy notice
7. unwritable profile

Every warning that an existing requirement mandates therefore still appears exactly as today: theme-selection's `--theme` warning, user-menu's malformed-file warning, and config-profile-directory's profile warnings. The configuration summary is shown only when none of them fires. In practice the conflicts are rare: the legacy and unwritable warnings fire only when there is no `config.toml` to warn about, and `error-diagnostics` logs every displaced warning once it lands.

Alternative considered: placing the summary after the profile-resolve warning (so it would outrank `--theme`). That would replace the profile-resolve and `--config-dir` warnings, which this change's own requirement says it must not do, and gives no clear benefit over a lower slot.

On a user-menu reload (`UserMenuReloaded`), `apply_user_menu_reload` raises the same dialog. It reports `malformed` as today, otherwise the warning summary, otherwise nothing.

Alternatives considered:

- **A multi-line or queued warning dialog.** This is a larger UI change to a surface that other in-flight work (config-profile-directory) also relies on. The summary is enough to find line N.
- **Warnings on the panel mini-status.** The status line is per-panel, overwritten by the next error, and easy to miss at startup.

When `error-diagnostics` lands a log sink, every warning, not just the summary, should also be logged.

### D5: Table-aware rewrites that keep comments and the BOM

`set_theme_line` and `set_panel_split_line` become thin wrappers over one `set_top_level_key(input, key, rendered_value)` built on `tokenize`. It:

- remembers whether the input started with a BOM and re-emits it;
- finds the first `KeyValue` with that key **before the first table header**, and replaces only its key and value text, re-attaching the original trailing comment (`theme = "purple-lights" # favourite`);
- when no top-level line exists, inserts `key = value` immediately **before the first table header**, or appends at the end when there is no table;
- leaves every other line byte-for-byte unchanged, as today.

Alternative considered: always appending, and relying on users to keep tables last. That is today's behavior, and it silently loses the setting the user just chose.

### D6: Durable writes through the shared helper

`save_theme_atomic`, `save_panel_split_atomic`, and `load_user_menu`'s first-run default write all call the shared durable-replace helper introduced by `editor-encoding-safety`:

- the temp file has a unique name in the same directory, so two instances never share `config.toml.tmp`;
- the temp file is flushed with `sync_all` before the replace;
- the temp file is removed on failure.

`history.json` uses the same helper, under `history-persistence-hardening`. The fixed-name `temp_sibling` helper is deleted once no caller remains.

theme-selection "Config write is atomic" and panel-split "Split persistence to configuration" (which says "the same atomic temp-file-and-rename write as other configuration updates") stay satisfied: the helper still writes a temporary file and atomically replaces the target with it (on Windows through `ReplaceFileW` when the target exists, per `editor-encoding-safety` D4), and every configuration write still shares one path.

### D7: Lossless history string decoding, top-level key lookup, BOM

- **One string decoder.** A single `decode_json_string(chars) -> Option<String>` replaces both copies of the escape loop, in `parse_history` and in `extract_string_field`:
  - `\uD800–\uDBFF` followed by `\uDC00–\uDFFF` decodes to the combined scalar;
  - a lone high or low surrogate, or a `\u` without four hex digits, decodes to U+FFFD, and decoding continues;
  - only a string with no closing quote fails, and then only that string is lost. Parsing resumes after it where the structure allows; the earlier and later entries are kept.
- **Key lookup.** `extract_value_span` finds the key by scanning the top-level object with the same string-literal awareness it already uses for bracket balancing. A `"directories"` sequence inside a string value can therefore never be mistaken for the key.
- **BOM.** `parse_history_file` (and the legacy `parse_history`) start with `strip_bom`.
- **No warnings.** None are produced for `history.json` (Non-Goals).

## Risks / Trade-offs

- **[Risk] A bare value that intentionally contains ` #`, such as `shell = cmd /C echo #1`, is now cut at the comment.** → This is TOML's documented behavior and is unlikely in practice. The README notes that values containing ` #` must be quoted. The change is listed under Compatibility in the proposal.
- **[Risk] Users who never saw warnings may now get a startup dialog for a long-standing harmless typo.** → This is intended: R-16 shows that silence hides real breakage. The dialog appears once per launch, is dismissed with any key, and names the line to fix.
- **[Trade-off] The one-row summary hides all but the first warning.** → The `(+N more)` count tells the user to look further, fixing the first and relaunching reveals the next, and `error-diagnostics` will add the full list to its log.
- **[Risk] With the earliest-qualifying-closing-quote rule, a verbatim `config.toml` value whose own text contains a quote followed by ` #` (for example `editor = "run "x" #2"`) is cut at that comment.** → This is a vanishingly rare shape. A line that ends in a quote with no comment, such as `editor = "a" b"`, still reads back verbatim as `a" b`, exactly as today. A value with no qualifying closing quote at all, such as `theme = "x`, is reported as malformed and the key keeps its default.
- **[Risk] A non-UTF-8 `usermenu.toml` cannot be repaired from inside FileCommand with the built-in editor**, because `editor-encoding-safety` redirects non-UTF-8 files to the viewer. → The warning says how to recover: `usermenu.toml is not UTF-8 text; using the default menu — re-save it as UTF-8 or delete it to recreate the defaults`. An external editor (`editor =`) can still open it.
- **[Trade-off] `sync_all` makes each theme or split write slower, and those writes still run on the UI thread.** Holding Ctrl+Left/Ctrl+Right issues one flushed write per step. → A flush of a file this small is typically a few milliseconds on a local disk. Moving the writes off the UI thread is `offload-blocking-io` (P-09); panel-split already allows rapid adjustments to coalesce writes.
- **[Risk] Rewrites now move a missing key above the first table instead of to the end.** → The resulting file reads back correctly, and every existing line keeps its bytes. Only the position of the new line differs.

## Migration Plan

No data migration is needed. Existing files parse identically unless they contain comments after values, a BOM, misplaced keys, or invalid lines. All of those were silently misread before and are now read as intended or reported. The README example is corrected in the same change.

## Open Questions

None.
