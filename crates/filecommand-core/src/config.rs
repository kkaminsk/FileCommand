//! Minimal `config.toml` reader plus the on-disk persistence helpers.
//!
//! Recognized keys: `splash` (bool), `animations` (bool, default true;
//! ui-animations), `theme` (string), `shell` (string),
//! `editor` (string, the F4 external-editor command), `panel_split`
//! (integer left-panel percentage; panel-split), and the overridable
//! bindings `key.paste_name` / `key.paste_path` / `key.quick_filter` /
//! `key.fuzzy_jump` / `key.split_left` / `key.split_right` /
//! `key.split_reset` / `key.clipboard_files` / `key.clipboard_paths` /
//! `key.clear_line`.
//! A `[mouse]` table holds `enabled` (bool, default true; mouse-input
//! "Mouse capture configuration") — the one section header this otherwise
//! flat, line-based parser recognizes, so `enabled` under `[mouse]` doesn't
//! collide with any bare top-level key of the same name. Top-level settings
//! must therefore come before the first `[table]`.
//!
//! `config.toml` and `usermenu.toml` are both read through one shared line
//! tokenizer ([`tokenize`]; configuration-files "Configuration files
//! tolerate a UTF-8 BOM and inline comments"): a leading UTF-8 BOM is
//! ignored, a `#` outside a quoted string starts a comment that runs to the
//! end of the line (on key lines and table headers alike), a `#` inside a
//! quoted value is kept, and in an unquoted `config.toml` value a `#` starts
//! a comment only at the start of the value or after whitespace, so a bare
//! `C:\Tools\C#\edit.exe` keeps working. Quoted `config.toml` values are
//! taken verbatim between the quotes (backslashes are not escapes).
//!
//! A missing `config.toml` is silent, but nothing else is ignored quietly:
//! a malformed line, an unknown key or table, a key under the wrong table,
//! an uninterpretable boolean or key binding, and a file that is not UTF-8
//! text each produce a [`ConfigWarning`] naming the file and line
//! (configuration-files "Malformed or unrecognized configuration lines
//! produce startup warnings"). The caller shows them once at startup as a
//! one-line summary ([`summarize_warnings`]). `theme` and `panel_split`
//! keep their specified silent fallbacks.
//!
//! Every write of `config.toml` and `usermenu.toml` goes through
//! [`crate::durable::replace_file`] (unique temp file, `sync_all`, rename),
//! and the theme/panel-split rewrites are table-aware: they replace only a
//! top-level line (keeping its trailing comment and any BOM) and insert a
//! missing key before the first table header (configuration-files
//! "Persisted configuration writes are durable", "Configuration rewrites
//! keep settings in the top-level table").
//!
//! Command history and the Ctrl+J directory-frecency index persist to
//! `history.json` next to the config, through the same durable helper
//! (unique temp file, `sync_all`, atomic replace) so an interrupted write
//! leaves either the previous or the new complete file. The document
//! carries a `"version": 2` marker: a file without it was written by an
//! earlier build whose visit times were session-relative, and
//! [`migrate_loaded_history`] re-stamps those at load. Before every write
//! the caller merges its snapshot with the file on disk
//! ([`merge_history_files`]), so two instances never erase each other's
//! history (history-persistence-hardening design D2, D5, D6). The reader
//! shares only the BOM strip with the TOML-like files; its JSON strings
//! decode losslessly (surrogate pairs combine, a bad `\u` escape becomes
//! U+FFFD) and never produce warnings.
//!
//! The F2 user menu is loaded from `usermenu.toml`, a real (if minimal)
//! TOML array-of-tables document — unlike `config.toml`/`history.json`
//! above, which are deliberately *not* real TOML/JSON parsers. Unlike those
//! two, malformed `usermenu.toml` content is treated as an error condition
//! (falling back to defaults and warning) rather than silently tolerated,
//! per user-menu "Create and recover the usermenu.toml file". A
//! `usermenu.toml` that is not UTF-8 text is malformed too, and is never
//! overwritten.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::quicksearch::{self, FrecencyEntry};
use crate::theme::DEFAULT_THEME_NAME;

/// The settings file in the profile directory.
pub const CONFIG_FILE: &str = "config.toml";

/// The default Windows shell. `cmd.exe` is chosen over PowerShell because
/// PowerShell costs 200 ms+ per spawn, which destroys the instant NC feel;
/// PowerShell is opt-in via `shell =`.
pub const DEFAULT_WINDOWS_SHELL: &str = "cmd.exe /C";
/// The default shell everywhere else.
pub const DEFAULT_UNIX_SHELL: &str = "/bin/sh -c";

/// The file command history is persisted to.
pub const HISTORY_FILE: &str = "history.json";

/// Newest-first history entries are capped at this many; older commands are
/// dropped rather than growing the file without bound.
pub const MAX_HISTORY: usize = 200;

/// One parsed key binding: modifier flags plus a normalized (lowercase) key
/// name such as `"enter"`, `"]"`, or `"f1"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

impl KeyBinding {
    pub fn new(ctrl: bool, alt: bool, shift: bool, key: &str) -> KeyBinding {
        KeyBinding { ctrl, alt, shift, key: key.to_lowercase() }
    }
}

/// Config-overridable bindings. Only the M3 paste bindings, plus M5's quick
/// filter and fuzzy jump, are overridable so far; the rest of the key map is
/// still compiled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    /// Pastes the cursor entry's file name. Default Ctrl+Enter — reliable on
    /// Windows (native console records), best-effort elsewhere.
    pub paste_name: KeyBinding,
    /// Pastes the cursor entry's full path. Default Ctrl+] (ASCII 0x1D),
    /// which every terminal delivers.
    pub paste_path: KeyBinding,
    /// Activates the Ctrl+P inline quick filter. Overridable because Ctrl+P
    /// deviates from classic Norton Commander, unlike the rest of the key
    /// map (quick-filter "Overridable binding"; design D6).
    pub quick_filter: KeyBinding,
    /// Opens the Ctrl+J fuzzy directory-jump dialog. Overridable for the
    /// same reason as `quick_filter` (fuzzy-jump "Overridden binding still
    /// opens the dialog"; design D6).
    pub fuzzy_jump: KeyBinding,
    /// Shrinks the left panel (moves the divider left) by
    /// `panel_split::SPLIT_STEP` columns. Default Ctrl+Left; overridable
    /// per the existing keymap convention (panel-split "Adjust and reset
    /// the panel split"; design D8).
    pub split_left: KeyBinding,
    /// Grows the left panel (moves the divider right). Default Ctrl+Right.
    pub split_right: KeyBinding,
    /// Resets the split to 50/50. Default Ctrl+=, the one binding in this
    /// table flagged as potentially undeliverable in some terminal hosts
    /// (design D8) — hence overridable here.
    pub split_reset: KeyBinding,
    /// Copies the F5-scope entries to the clipboard as file objects.
    /// Default Ctrl+C (clipboard-export "Clipboard key bindings"). Ctrl+Ins
    /// is a fixed alias, not rebindable through this key.
    pub clipboard_files: KeyBinding,
    /// Copies the F5-scope entries' absolute paths to the clipboard as
    /// text. Default Ctrl+Shift+Ins.
    pub clipboard_paths: KeyBinding,
    /// Clears the whole command line, ending any history recall in
    /// progress, while the panels own input and the buffer is non-empty.
    /// Default Ctrl+Y (command-line "Clear the command line with a
    /// dedicated key"; command-line-clear-key design D1).
    pub clear_line: KeyBinding,
}

impl Default for Keys {
    fn default() -> Self {
        Keys {
            paste_name: KeyBinding::new(true, false, false, "enter"),
            paste_path: KeyBinding::new(true, false, false, "]"),
            quick_filter: KeyBinding::new(true, false, false, "p"),
            fuzzy_jump: KeyBinding::new(true, false, false, "j"),
            split_left: KeyBinding::new(true, false, false, "left"),
            split_right: KeyBinding::new(true, false, false, "right"),
            split_reset: KeyBinding::new(true, false, false, "="),
            clipboard_files: KeyBinding::new(true, false, false, "c"),
            clipboard_paths: KeyBinding::new(true, false, true, "insert"),
            clear_line: KeyBinding::new(true, false, false, "y"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub splash: bool,
    /// Whether the decorative UI animations run (ui-animations "Animations
    /// are disabled by config or launch flag"). Defaults to `true`;
    /// `animations = false` here or `--noanimations` at launch turns every
    /// surface back into its final frame.
    pub animations: bool,
    pub theme: String,
    /// `shell = ` verbatim. `None` means "use the platform default"; see
    /// [`crate::shell::resolve_shell`].
    pub shell: Option<String>,
    /// `editor = ` verbatim, the F4 external-editor command. `None` (also
    /// yielded by a blank/empty value) means unset: F4 shows a "no editor
    /// configured" message instead of spawning anything (external-editor:
    /// Config-driven external editor command — "Editor command unset").
    pub editor: Option<String>,
    /// The persisted vertical panel split, as a left-panel percentage
    /// (panel-split "Split persistence to configuration"). An unset,
    /// non-integer, or out-of-`[MIN_PANEL_PERCENT, MAX_PANEL_PERCENT]`
    /// value falls back to `panel_split::DEFAULT_SPLIT_PERCENT`.
    pub panel_split: u16,
    /// `[mouse]` table's `enabled` key: whether the terminal guard issues
    /// mouse-capture escape sequences at all. Defaults to `true`; overridden
    /// off by `[mouse] enabled = false` here or by `--nomouse` at launch
    /// (mouse-input "Mouse capture configuration").
    pub mouse_enabled: bool,
    pub keys: Keys,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            splash: true,
            animations: true,
            theme: DEFAULT_THEME_NAME.to_string(),
            shell: None,
            editor: None,
            panel_split: crate::panel_split::DEFAULT_SPLIT_PERCENT,
            mouse_enabled: true,
            keys: Keys::default(),
        }
    }
}

/// Parse a modifier-prefixed binding such as `"ctrl+enter"` or `"alt+shift+f3"`.
/// Returns `None` when no key name remains after the modifiers.
pub fn parse_binding(value: &str) -> Option<KeyBinding> {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut key: Option<String> = None;
    for part in value.split('+') {
        let part = part.trim();
        if part.is_empty() {
            // A trailing `+` means the key itself is `+`, e.g. "ctrl++".
            key = Some("+".to_string());
            continue;
        }
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "meta" => alt = true,
            "shift" => shift = true,
            other => key = Some(other.to_string()),
        }
    }
    key.map(|key| KeyBinding { ctrl, alt, shift, key })
}

// ---------------------------------------------------------------------
// Shared line tokenizer for config.toml and usermenu.toml
// (configuration-files; design D1)
// ---------------------------------------------------------------------

/// Remove one leading U+FEFF (the UTF-8 byte-order mark Notepad and friends
/// write), so the first line reads exactly as if the mark were absent
/// (configuration-files "Configuration files tolerate a UTF-8 BOM and inline
/// comments").
pub fn strip_bom(input: &str) -> &str {
    input.strip_prefix('\u{feff}').unwrap_or(input)
}

/// How a basic (`"..."`) string's closing quote is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteMode {
    /// `config.toml`: the text between the quotes is verbatim, so a
    /// backslash never escapes the quote (`"C:\dir\"` terminates).
    Verbatim,
    /// `usermenu.toml`: real TOML, where `\"` does not close the string.
    TomlEscapes,
}

/// The raw, uninterpreted value of a `key = value` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// The inner text of `"…"`, not unescaped.
    Basic(String),
    /// The inner text of `'…'`.
    Literal(String),
    /// Unquoted text, surrounding whitespace trimmed.
    Bare(String),
}

impl Value {
    /// The value's text, whichever way it was quoted.
    pub fn text(&self) -> &str {
        match self {
            Value::Basic(s) | Value::Literal(s) | Value::Bare(s) => s,
        }
    }
}

/// What one line of a TOML-like file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    /// Empty, whitespace, or a whole-line comment.
    Blank,
    /// `[name]` (or `[[name]]` when `array`), optionally followed by a
    /// comment.
    Table { name: String, array: bool },
    /// `key = value`, with the trailing `# …` comment text (from the `#`)
    /// kept for the rewriter.
    KeyValue { key: String, value: Value, comment: Option<String> },
    /// No `=`, an empty key, an unterminated quote, or stray text after a
    /// closing quote or table header.
    Malformed { reason: String },
}

/// One tokenized line with its 1-based number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub number: usize,
    pub kind: LineKind,
}

/// Tokenize a TOML-like document into lines. The caller strips the BOM
/// first ([`strip_bom`]). Comment rules (design D1): a `#` outside quotes
/// starts a comment on key lines and table headers; a `#` in a bare value
/// starts a comment only at the value start or after whitespace; the
/// closing quote of a quoted value is the earliest matching quote that
/// leaves only whitespace or a `#` comment behind it.
pub fn tokenize(input: &str, mode: QuoteMode) -> Vec<Line> {
    input.lines().enumerate().map(|(i, raw)| Line { number: i + 1, kind: tokenize_line(raw, mode) }).collect()
}

fn tokenize_line(raw: &str, mode: QuoteMode) -> LineKind {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') {
        return LineKind::Blank;
    }
    if line.starts_with('[') {
        return tokenize_header(line);
    }
    let Some((key, rest)) = line.split_once('=') else {
        return LineKind::Malformed { reason: "expected key = value".to_string() };
    };
    let key = key.trim();
    if key.is_empty() {
        return LineKind::Malformed { reason: "expected a key before =".to_string() };
    }
    let rest = rest.trim_start();
    let quote = match rest.chars().next() {
        Some(q @ ('"' | '\'')) => Some(q),
        _ => None,
    };
    match quote {
        Some(q) => match find_closing_quote(rest, q, mode) {
            Some(end) => {
                let inner = rest[1..end].to_string();
                let value = if q == '"' { Value::Basic(inner) } else { Value::Literal(inner) };
                let tail = rest[end + 1..].trim();
                let comment = if tail.is_empty() { None } else { Some(tail.to_string()) };
                LineKind::KeyValue { key: key.to_string(), value, comment }
            }
            None => {
                let reason = if rest[1..].contains(q) { "unexpected text after the closing quote" } else { "unterminated quoted value" };
                LineKind::Malformed { reason: reason.to_string() }
            }
        },
        None => {
            let (value, comment) = split_bare_comment(rest);
            LineKind::KeyValue { key: key.to_string(), value: Value::Bare(value.trim().to_string()), comment: comment.map(str::to_string) }
        }
    }
}

/// `[name]` / `[[name]]`, with an optional trailing comment.
fn tokenize_header(line: &str) -> LineKind {
    let (array, open) = if line.starts_with("[[") { (true, "[[") } else { (false, "[") };
    let close = if array { "]]" } else { "]" };
    let body = &line[open.len()..];
    let Some(end) = body.find(close) else {
        return LineKind::Malformed { reason: format!("expected a closing {close} after the table name") };
    };
    let name = body[..end].trim().to_string();
    let tail = body[end + close.len()..].trim_start();
    if !tail.is_empty() && !tail.starts_with('#') {
        return LineKind::Malformed { reason: "unexpected text after the table header".to_string() };
    }
    LineKind::Table { name, array }
}

/// The byte index of the closing quote in `rest` (which starts with `q`):
/// the earliest `q` after which only whitespace or a `#` comment remains.
/// In [`QuoteMode::TomlEscapes`] a backslash escapes the next character.
fn find_closing_quote(rest: &str, q: char, mode: QuoteMode) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in rest.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if mode == QuoteMode::TomlEscapes && q == '"' && c == '\\' {
            escaped = true;
            continue;
        }
        if c == q {
            let tail = rest[i + 1..].trim_start();
            if tail.is_empty() || tail.starts_with('#') {
                return Some(i);
            }
        }
    }
    None
}

/// Split a bare value from its trailing comment: a `#` counts only at the
/// start of the value or after whitespace, so `C:\Tools\C#\edit.exe` stays
/// whole.
fn split_bare_comment(rest: &str) -> (&str, Option<&str>) {
    let mut prev_space = true;
    for (i, c) in rest.char_indices() {
        if c == '#' && prev_space {
            return (&rest[..i], Some(&rest[i..]));
        }
        prev_space = c.is_whitespace();
    }
    (rest, None)
}

/// One configuration problem, tied to a file and (when `line > 0`) a line
/// (configuration-files "Malformed or unrecognized configuration lines
/// produce startup warnings"; design D2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWarning {
    pub file: &'static str,
    /// 1-based line number; `0` for a whole-file problem (not UTF-8,
    /// unreadable).
    pub line: usize,
    pub message: String,
}

impl ConfigWarning {
    fn at(file: &'static str, line: usize, message: impl Into<String>) -> ConfigWarning {
        ConfigWarning { file, line, message: message.into() }
    }

    fn whole_file(file: &'static str, message: impl Into<String>) -> ConfigWarning {
        ConfigWarning { file, line: 0, message: message.into() }
    }
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{} {}", self.file, self.message)
        } else {
            write!(f, "{} line {}: {}", self.file, self.line, self.message)
        }
    }
}

/// The one-row startup-dialog summary of a warning list: the first warning,
/// plus ` (+N more)` when there are others. `None` for an empty list
/// (design D4).
pub fn summarize_warnings<W: fmt::Display>(warnings: &[W]) -> Option<String> {
    let first = warnings.first()?;
    match warnings.len() {
        1 => Some(first.to_string()),
        n => Some(format!("{first} (+{} more)", n - 1)),
    }
}

/// Decode file bytes as UTF-8 text. A NUL byte marks the file as not text
/// in this encoding (a UTF-16 file without its BOM would otherwise pass as
/// UTF-8 full of NULs).
fn decode_utf8_text(bytes: Vec<u8>) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

// ---------------------------------------------------------------------
// config.toml
// ---------------------------------------------------------------------

/// A parsed `config.toml`: the settings plus every line that was not read
/// the way it was written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedConfig {
    pub config: Config,
    pub warnings: Vec<ConfigWarning>,
}

/// Which table the parser is inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigTable {
    TopLevel,
    Mouse,
    /// An unrecognized header; its keys were covered by the header warning.
    Unknown,
}

/// Parse a `config.toml` document. Blank lines, comments, a BOM, and
/// trailing comments are accepted silently; everything that is ignored or
/// falls back to a default is reported in `warnings` (design D2). The
/// `[mouse]` table is the one recognized header: once seen, `key = value`
/// lines belong to it (only `enabled` is read there) until the next header
/// (mouse-input "Mouse capture configuration").
pub fn parse(input: &str) -> ParsedConfig {
    let mut config = Config::default();
    let mut warnings = Vec::new();
    let mut table = ConfigTable::TopLevel;
    for line in tokenize(strip_bom(input), QuoteMode::Verbatim) {
        let n = line.number;
        match line.kind {
            LineKind::Blank => {}
            LineKind::Malformed { reason } => warnings.push(ConfigWarning::at(CONFIG_FILE, n, format!("{reason}; line ignored"))),
            LineKind::Table { name, array } => {
                if name == "mouse" && !array {
                    table = ConfigTable::Mouse;
                } else {
                    let header = if array { format!("[[{name}]]") } else { format!("[{name}]") };
                    warnings.push(ConfigWarning::at(CONFIG_FILE, n, format!("unknown table {header}; its settings are ignored")));
                    table = ConfigTable::Unknown;
                }
            }
            LineKind::KeyValue { key, value, .. } => match table {
                ConfigTable::Unknown => {}
                ConfigTable::Mouse => {
                    if key == "enabled" {
                        apply_bool(&value, &mut config.mouse_enabled, "enabled", n, &mut warnings);
                    } else {
                        warnings.push(ConfigWarning::at(
                            CONFIG_FILE,
                            n,
                            format!("\"{key}\" is not a [mouse] setting and was ignored — put top-level settings before the first [table]"),
                        ));
                    }
                }
                ConfigTable::TopLevel => apply_top_level_key(&mut config, &key, &value, n, &mut warnings),
            },
        }
    }
    ParsedConfig { config, warnings }
}

fn apply_top_level_key(config: &mut Config, key: &str, value: &Value, line: usize, warnings: &mut Vec<ConfigWarning>) {
    let binding_target: Option<&mut KeyBinding> = match key {
        "key.paste_name" => Some(&mut config.keys.paste_name),
        "key.paste_path" => Some(&mut config.keys.paste_path),
        "key.quick_filter" => Some(&mut config.keys.quick_filter),
        "key.fuzzy_jump" => Some(&mut config.keys.fuzzy_jump),
        "key.split_left" => Some(&mut config.keys.split_left),
        "key.split_right" => Some(&mut config.keys.split_right),
        "key.split_reset" => Some(&mut config.keys.split_reset),
        "key.clipboard_files" => Some(&mut config.keys.clipboard_files),
        "key.clipboard_paths" => Some(&mut config.keys.clipboard_paths),
        "key.clear_line" => Some(&mut config.keys.clear_line),
        _ => None,
    };
    if let Some(target) = binding_target {
        match parse_string(value).as_deref().and_then(parse_binding) {
            Some(binding) => *target = binding,
            None => {
                warnings.push(ConfigWarning::at(CONFIG_FILE, line, format!("{key} \"{}\" is not a key binding; using the default", value.text())))
            }
        }
        return;
    }
    match key {
        "splash" => apply_bool(value, &mut config.splash, key, line, warnings),
        "animations" => apply_bool(value, &mut config.animations, key, line, warnings),
        "theme" => {
            // Silent fallback for an unknown name is theme-selection's rule
            // ("Unknown configured theme falls back to default"); only an
            // empty value is treated as unset here, as before.
            if let Some(s) = parse_string(value) {
                config.theme = s;
            }
        }
        "shell" => {
            if let Some(s) = parse_string(value) {
                if !s.trim().is_empty() {
                    config.shell = Some(s);
                }
            }
        }
        "editor" => {
            if let Some(s) = parse_string(value) {
                if !s.trim().is_empty() {
                    config.editor = Some(s);
                }
            }
        }
        "panel_split" => {
            // Tolerant: a non-integer or out-of-range value is treated as
            // though the key were unset — the default (50) is already in
            // `config.panel_split` from `Config::default` (panel-split
            // "Invalid configured value falls back"). No warning, by that
            // requirement.
            if let Value::Bare(text) = value {
                if let Ok(percent) = text.parse::<u16>() {
                    if percent <= 100 {
                        config.panel_split = percent;
                    }
                }
            }
        }
        _ => warnings.push(ConfigWarning::at(CONFIG_FILE, line, format!("unknown setting \"{key}\"; ignored"))),
    }
}

/// Read a bare `true`/`false` into `target`, warning (and keeping the
/// default) for anything else.
fn apply_bool(value: &Value, target: &mut bool, key: &str, line: usize, warnings: &mut Vec<ConfigWarning>) {
    match parse_bool(value) {
        Some(b) => *target = b,
        None => warnings.push(ConfigWarning::at(CONFIG_FILE, line, format!("{key} must be true or false; using the default"))),
    }
}

fn parse_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bare(text) if text == "true" => Some(true),
        Value::Bare(text) if text == "false" => Some(false),
        _ => None,
    }
}

/// Accept both quoted (`"cmd.exe /C"`) and bare (`cmd.exe /C`) values — the
/// shell spec is far more readable unquoted, and this parser has never been
/// strict TOML anyway. Quoted text is taken verbatim (no escapes), so
/// `editor = "C:\Tools\ed.exe"` keeps working. An empty value is `None`.
fn parse_string(value: &Value) -> Option<String> {
    let text = value.text();
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

/// Load `config.toml` from `path`. A missing file is not an error and not a
/// warning: defaults apply silently. A file that exists but is not UTF-8
/// text, or cannot be read for another reason, also yields defaults, with
/// one whole-file warning (design D2).
pub fn load(path: &Path) -> ParsedConfig {
    match std::fs::read(path) {
        Ok(bytes) => match decode_utf8_text(bytes) {
            Some(contents) => parse(&contents),
            None => ParsedConfig {
                config: Config::default(),
                warnings: vec![ConfigWarning::whole_file(CONFIG_FILE, "is not UTF-8 text; using default settings")],
            },
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => ParsedConfig::default(),
        Err(e) => ParsedConfig {
            config: Config::default(),
            warnings: vec![ConfigWarning::whole_file(CONFIG_FILE, format!("could not be read ({e}); using default settings"))],
        },
    }
}

// ---------------------------------------------------------------------
// config.toml theme persistence (visual-themes)
// ---------------------------------------------------------------------

/// Rewrite the top-level `key = <rendered>` line of a `config.toml`
/// document (configuration-files "Configuration rewrites keep settings in
/// the top-level table"; design D5):
///
/// - the first `key` line *before the first table header* is replaced,
///   keeping its trailing comment;
/// - when there is none, `key = <rendered>` is inserted immediately before
///   the first table header, or appended when the file has no table — so
///   the new line can never land inside `[mouse]` and be ignored on the
///   next start;
/// - a leading BOM is re-emitted, and every other line is left unchanged.
///
/// Line endings follow the file: CRLF when the input uses CRLF, LF
/// otherwise.
pub fn set_top_level_key(input: &str, key: &str, rendered: &str) -> String {
    let body = strip_bom(input);
    let bom = &input[..input.len() - body.len()];
    let eol = if body.contains("\r\n") { "\r\n" } else { "\n" };
    let raw_lines: Vec<&str> = body.lines().collect();
    let mut first_table = None;
    let mut target = None;
    for line in tokenize(body, QuoteMode::Verbatim) {
        match line.kind {
            LineKind::Table { .. } => {
                first_table = Some(line.number - 1);
                break;
            }
            LineKind::KeyValue { key: k, comment, .. } if k == key && target.is_none() => target = Some((line.number - 1, comment)),
            _ => {}
        }
    }
    let render = |comment: Option<&str>| match comment {
        Some(c) => format!("{key} = {rendered} {c}"),
        None => format!("{key} = {rendered}"),
    };
    let mut out = String::with_capacity(input.len() + key.len() + rendered.len() + 8);
    out.push_str(bom);
    let insert_at = match target {
        Some(_) => None,
        None => Some(first_table.unwrap_or(raw_lines.len())),
    };
    for (i, raw) in raw_lines.iter().enumerate() {
        if insert_at == Some(i) {
            out.push_str(&render(None));
            out.push_str(eol);
        }
        match &target {
            Some((at, comment)) if *at == i => out.push_str(&render(comment.as_deref())),
            _ => out.push_str(raw),
        }
        out.push_str(eol);
    }
    if insert_at == Some(raw_lines.len()) {
        out.push_str(&render(None));
        out.push_str(eol);
    }
    out
}

/// Rewrite `input`'s top-level `theme = "<name>"` line to `theme_name` via
/// [`set_top_level_key`], so applying a theme never clobbers the rest of a
/// hand-edited `config.toml` (theme-selection "Applied theme persists to
/// configuration").
pub fn set_theme_line(input: &str, theme_name: &str) -> String {
    set_top_level_key(input, "theme", &toml_quote(theme_name))
}

/// Read the current `config.toml` text for a rewrite. A missing file is
/// empty (matching [`load`]'s tolerance); a file that is not UTF-8 text is
/// refused rather than replaced by a one-line file, because the rewrite
/// could not keep its other settings.
fn read_config_for_rewrite(path: &Path) -> io::Result<String> {
    match std::fs::read(path) {
        Ok(bytes) => decode_utf8_text(bytes)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("{CONFIG_FILE} is not UTF-8 text; not rewritten"))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

/// Write `updated` to `config.toml` at `path` through the shared durable
/// helper: a uniquely named temp file, `sync_all`, then an atomic replace,
/// with the temp file removed on failure (configuration-files "Persisted
/// configuration writes are durable"; theme-selection "Config write is
/// atomic"; panel-split "Config write is atomic").
fn write_config_durably(path: &Path, updated: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    crate::durable::replace_file(path, updated.as_bytes())
}

/// Write the updated `theme =` key to `config.toml` at `path` durably (see
/// [`write_config_durably`]). Reads the existing file so every other line
/// survives the round trip.
pub fn save_theme_atomic(path: &Path, theme_name: &str) -> io::Result<()> {
    let existing = read_config_for_rewrite(path)?;
    write_config_durably(path, &set_theme_line(&existing, theme_name))
}

// ---------------------------------------------------------------------
// config.toml panel-split persistence (panel-split)
// ---------------------------------------------------------------------

/// Rewrite `input`'s top-level `panel_split = <percent>` line to `percent`
/// via [`set_top_level_key`], mirroring [`set_theme_line`] exactly
/// (panel-split "Split persistence to configuration").
pub fn set_panel_split_line(input: &str, percent: u16) -> String {
    set_top_level_key(input, "panel_split", &percent.to_string())
}

/// Write the updated `panel_split =` key to `config.toml` at `path` through
/// the same durable write-back path as [`save_theme_atomic`] (panel-split
/// "Config write is atomic").
pub fn save_panel_split_atomic(path: &Path, percent: u16) -> io::Result<()> {
    let existing = read_config_for_rewrite(path)?;
    write_config_durably(path, &set_panel_split_line(&existing, percent))
}

// ---------------------------------------------------------------------
// history.json
// ---------------------------------------------------------------------

/// Render a JSON array of strings, without a trailing newline (shared by
/// [`render_history`] and the `"commands"` array inside
/// [`render_history_file`]).
fn render_string_array(entries: &[String]) -> String {
    let mut out = String::from("[");
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('\n');
        out.push_str("  ");
        out.push_str(&json_escape(entry));
    }
    if !entries.is_empty() {
        out.push('\n');
    }
    out.push(']');
    out
}

/// Render history entries as a JSON array of strings. Hand-rolled rather
/// than pulled from `serde` — this is the only JSON in the workspace and the
/// shape is one string array.
pub fn render_history(entries: &[String]) -> String {
    let mut out = render_string_array(entries);
    out.push('\n');
    out
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Parse a JSON array of strings. Tolerant: anything that isn't a
/// well-formed string array yields whatever strings could be read, so a
/// corrupt history file degrades to a shorter history rather than an error.
/// A leading BOM is ignored and string escapes decode losslessly
/// ([`decode_json_string`]), so no entry is lost to an escape the reader
/// does not understand (configuration-files "History file strings decode
/// without losing entries").
pub fn parse_history(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = strip_bom(input).chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        match decode_json_string(&mut chars) {
            Some(s) => out.push(s),
            None => break,
        }
    }
    out
}

/// Decode the body of a JSON string literal; `chars` is positioned just
/// after the opening quote and is left just after the closing quote. Returns
/// `None` only when the string is unterminated (design D7):
///
/// - a `\uD800`–`\uDBFF` escape followed by a `\uDC00`–`\uDFFF` escape
///   decodes to the one character they encode;
/// - a lone surrogate, or a `\u` without four hex digits, decodes to U+FFFD
///   and decoding continues;
/// - other escapes follow JSON (`\n`, `\r`, `\t`, `\b`, `\f`, `\"`, `\\`,
///   `\/`); an unknown `\x` yields `x`, as before.
fn decode_json_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let mut s = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(s),
            '\\' => match chars.next()? {
                'n' => s.push('\n'),
                'r' => s.push('\r'),
                't' => s.push('\t'),
                'b' => s.push('\u{8}'),
                'f' => s.push('\u{c}'),
                'u' => s.push(decode_unicode_escape(chars)),
                other => s.push(other),
            },
            c => s.push(c),
        }
    }
}

/// The four hex digits following `\u`. Nothing is consumed unless all four
/// are present, so a short escape (`\u12"`) neither swallows the closing
/// quote nor loses the characters after the `\u`.
fn take_hex4(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<u32> {
    let mut ahead = chars.clone();
    let mut value = 0u32;
    for _ in 0..4 {
        let digit = ahead.next()?.to_digit(16)?;
        value = value * 16 + digit;
    }
    *chars = ahead;
    Some(value)
}

/// Decode the code unit(s) after a `\u` into one character, combining a
/// surrogate pair and substituting U+FFFD for anything that is not a scalar.
fn decode_unicode_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> char {
    let Some(unit) = take_hex4(chars) else { return '\u{fffd}' };
    if (0xD800..=0xDBFF).contains(&unit) {
        // A high surrogate: look ahead for `\u` + low surrogate without
        // consuming anything unless the pair is complete.
        let mut ahead = chars.clone();
        if ahead.next() == Some('\\') && ahead.next() == Some('u') {
            if let Some(low) = take_hex4(&mut ahead) {
                if (0xDC00..=0xDFFF).contains(&low) {
                    *chars = ahead;
                    let scalar = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                    return char::from_u32(scalar).unwrap_or('\u{fffd}');
                }
            }
        }
        return '\u{fffd}';
    }
    char::from_u32(unit).unwrap_or('\u{fffd}')
}

/// Read persisted history. A missing or unreadable file is an empty
/// history, never an error.
pub fn load_history(path: &Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => parse_history(&contents),
        Err(_) => Vec::new(),
    }
}

/// Write the plain-array history durably through
/// [`crate::durable::replace_file`] (unique temp file, `sync_all`, atomic
/// replace), so a reader never observes a partial write and no two
/// instances share a temp file name.
pub fn save_history_atomic(path: &Path, entries: &[String]) -> io::Result<()> {
    ensure_parent_dir(path)?;
    crate::durable::replace_file(path, render_history(entries).as_bytes())
}

/// Create `path`'s parent directory when it is missing, so a first write
/// into a fresh profile directory succeeds.
fn ensure_parent_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

/// Append `command` as the newest history entry, dropping any earlier
/// identical entry so recall walks distinct commands, and trimming to
/// [`MAX_HISTORY`].
pub fn push_history(history: &mut Vec<String>, command: &str) {
    if command.trim().is_empty() {
        return;
    }
    history.retain(|h| h != command);
    history.push(command.to_string());
    if history.len() > MAX_HISTORY {
        let excess = history.len() - MAX_HISTORY;
        history.drain(0..excess);
    }
}

// ---------------------------------------------------------------------
// history.json (combined form): command history + directory frecency
// ---------------------------------------------------------------------
//
// `fuzzy-jump`'s visited-directory frecency index shares `history.json`
// with the plain command-history array above (design D6; fuzzy-jump
// "Directory history persistence"). The two live under top-level
// `"commands"` and `"directories"` keys in one JSON object rather than as a
// bare array. [`HistoryFile`] and the functions below are additive: wiring
// the live app to read/write this combined shape (in place of the plain
// array `render_history`/`load_history` above) is a `filecommand-tui`
// concern for a later step.

/// One on-disk `history.json` document: command-line history plus the
/// `fuzzy-jump` directory-frecency index, sharing one file and one atomic
/// write (fuzzy-jump "Directory history persistence").
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HistoryFile {
    pub commands: Vec<String>,
    pub directories: Vec<FrecencyEntry>,
}

/// The `"version"` a `history.json` written by this build carries. A file
/// without the key predates wall-clock visit times (design D2).
pub const HISTORY_FORMAT_VERSION: u32 = 2;

/// Render a [`HistoryFile`] to its on-disk JSON object form, with the
/// format marker first.
pub fn render_history_file(file: &HistoryFile) -> String {
    let mut out = format!("{{\n  \"version\": {HISTORY_FORMAT_VERSION},\n  \"commands\": ");
    out.push_str(&render_string_array(&file.commands));
    out.push_str(",\n  \"directories\": [");
    for (i, d) in file.directories.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("\n    {\"path\": ");
        out.push_str(&json_escape(&d.path.to_string_lossy()));
        out.push_str(&format!(", \"visit_count\": {}, \"last_visited_ms\": {}}}", d.visit_count, d.last_visited_ms));
    }
    if !file.directories.is_empty() {
        out.push_str("\n  ");
    }
    out.push_str("]\n}\n");
    out
}

/// Parse a `history.json` document in its combined object form. Tolerant:
/// a missing or unparsable `"commands"`/`"directories"` key yields an empty
/// list for that key rather than failing the whole parse, so a partially
/// malformed or hand-edited file degrades gracefully (fuzzy-jump "Missing
/// or malformed history file").
pub fn parse_history_file(input: &str) -> HistoryFile {
    parse_history_document(input).file
}

/// A parsed `history.json` plus whether it predates the format marker
/// (design D2): a `legacy` file's `last_visited_ms` values are
/// session-relative and must be re-stamped by [`migrate_loaded_history`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HistoryLoad {
    pub file: HistoryFile,
    pub legacy: bool,
}

/// [`parse_history_file`] that also reports whether the top-level
/// `"version"` key was absent. The key is found with the same top-level-only
/// lookup as the arrays, so a `"version"` inside a command string is never
/// mistaken for the marker. Tolerance for malformed content is unchanged: a
/// file that is not an object at all is empty history, and `legacy`, which
/// is harmless for an empty file.
pub fn parse_history_document(input: &str) -> HistoryLoad {
    let input = strip_bom(input);
    let commands = extract_value_span(input, "commands").map(parse_history).unwrap_or_default();
    let directories = extract_value_span(input, "directories").map(parse_directories).unwrap_or_default();
    let legacy = top_level_value(input, "version").is_none();
    HistoryLoad { file: HistoryFile { commands, directories }, legacy }
}

/// Read a combined `history.json`. A missing or unreadable file, or one
/// that fails to parse at all, is empty history — never an error (fuzzy-jump
/// "Missing or malformed history file").
pub fn load_history_file(path: &Path) -> HistoryFile {
    load_history_document(path).file
}

/// [`load_history_file`] that also reports the legacy flag (see
/// [`parse_history_document`]). A missing file is empty and not legacy:
/// there is nothing to migrate.
pub fn load_history_document(path: &Path) -> HistoryLoad {
    match std::fs::read_to_string(path) {
        Ok(contents) => parse_history_document(&contents),
        Err(_) => HistoryLoad::default(),
    }
}

/// Bring a just-loaded history up to date (fuzzy-jump "Legacy history file
/// is migrated at load", "Future last-visit time is clamped"; design D2):
///
/// - for a `legacy` file, every directory's `last_visited_ms` becomes
///   `now_epoch`, so established directories rank by visit count and then
///   age normally;
/// - for any file, a `last_visited_ms` in the future (a corrected system
///   clock) is clamped to `now_epoch`;
/// - the directory list is pruned to [`quicksearch::MAX_DIR_HISTORY`].
pub fn migrate_loaded_history(file: &mut HistoryFile, legacy: bool, now_epoch: u64) {
    for entry in &mut file.directories {
        if legacy || entry.last_visited_ms > now_epoch {
            entry.last_visited_ms = now_epoch;
        }
    }
    quicksearch::prune_to_cap(&mut file.directories, quicksearch::MAX_DIR_HISTORY, now_epoch);
}

/// Combine the `history.json` on disk with this instance's snapshot before a
/// write, so concurrently running instances do not discard each other's
/// entries (fuzzy-jump "Two instances keep each other's history"; design
/// D5):
///
/// - directories are the union by path — a path in both keeps the larger
///   visit count and the later visit time — this instance's entries first in
///   their order, then disk-only entries in disk order, pruned to
///   [`quicksearch::MAX_DIR_HISTORY`];
/// - commands present only on disk come first, in disk order, then
///   `mine.commands`, with the oldest dropped to fit [`MAX_HISTORY`].
///
/// `max` is idempotent, so repeated flushes never inflate counts.
pub fn merge_history_files(disk: &HistoryFile, mine: &HistoryFile, now_epoch: u64) -> HistoryFile {
    let mut directories = mine.directories.clone();
    for theirs in &disk.directories {
        match directories.iter_mut().find(|e| e.path == theirs.path) {
            Some(ours) => {
                ours.visit_count = ours.visit_count.max(theirs.visit_count);
                ours.last_visited_ms = ours.last_visited_ms.max(theirs.last_visited_ms);
            }
            None => directories.push(theirs.clone()),
        }
    }
    quicksearch::prune_to_cap(&mut directories, quicksearch::MAX_DIR_HISTORY, now_epoch);

    let mut commands: Vec<String> = disk.commands.iter().filter(|c| !mine.commands.contains(c)).cloned().collect();
    commands.extend(mine.commands.iter().cloned());
    if commands.len() > MAX_HISTORY {
        let excess = commands.len() - MAX_HISTORY;
        commands.drain(0..excess);
    }
    HistoryFile { commands, directories }
}

/// Write a combined `history.json` durably through
/// [`crate::durable::replace_file`]: a uniquely named temp file in the same
/// directory, `sync_all`, then an atomic replace, with the temp file removed
/// on failure (fuzzy-jump "Interrupted write leaves a complete file"; design
/// D6).
pub fn save_history_file_atomic(path: &Path, file: &HistoryFile) -> io::Result<()> {
    ensure_parent_dir(path)?;
    crate::durable::replace_file(path, render_history_file(file).as_bytes())
}

/// Find the value of top-level `key` in the JSON object `object` and return
/// the text from its first character onward. Only keys of the outermost
/// object are considered, and string literals are skipped, so `"commands"`
/// inside a command string can never be mistaken for the key
/// (configuration-files "Key text inside a string is not mistaken for a
/// key"; design D7). `None` when the text is not an object or the key is
/// absent.
fn top_level_value<'a>(object: &'a str, key: &str) -> Option<&'a str> {
    let object = object.trim_start();
    let bytes = object.as_bytes();
    if bytes.first() != Some(&b'{') {
        return None;
    }
    // JSON's structural characters are ASCII, so a byte scan is exact and
    // every index below lands on a char boundary.
    let mut i = 1;
    let mut depth = 1i32;
    let mut expect_key = true;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i + 1;
                let end = json_string_end(bytes, start)?;
                i = end + 1;
                if depth == 1 && expect_key {
                    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                        i += 1;
                    }
                    if bytes.get(i) != Some(&b':') {
                        return None;
                    }
                    i += 1;
                    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                        i += 1;
                    }
                    // The keys this file uses are plain ASCII, so the raw
                    // literal is compared without unescaping.
                    if &object[start..end] == key {
                        return Some(&object[i..]);
                    }
                    expect_key = false;
                }
            }
            b'{' | b'[' => {
                depth += 1;
                i += 1;
            }
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return None;
                }
                i += 1;
            }
            b',' if depth == 1 => {
                expect_key = true;
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// The index of the quote that closes the JSON string whose body starts at
/// `start`, skipping backslash escapes. `None` when unterminated.
fn json_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Find top-level `"key": <value>` in `input` and return the raw text of
/// the balanced `[...]` or `{...}` that follows, honoring string-literal
/// quoting (so a bracket inside a quoted path never confuses the balance
/// count). `None` when the key is absent or its value isn't a bracketed
/// span.
fn extract_value_span<'a>(input: &'a str, key: &str) -> Option<&'a str> {
    let value = top_level_value(input, key)?;
    let open = value.chars().next()?;
    let close = match open {
        '[' => ']',
        '{' => '}',
        _ => return None,
    };
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut end = None;
    for (i, c) in value.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    Some(&value[..=end?])
}

/// Split a `[...]`-spanned array body into its top-level `{...}` object
/// substrings, honoring string-literal quoting.
fn split_top_level_objects(array_span: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut start = None;
    for (i, c) in array_span.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start.take() {
                        out.push(&array_span[s..=i]);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Extract a quoted string field `"key": "value"` from within one object
/// substring, decoding it with the same [`decode_json_string`] as
/// [`parse_history`].
fn extract_string_field(obj: &str, key: &str) -> Option<String> {
    let value = top_level_value(obj, key)?;
    let mut chars = value.strip_prefix('"')?.chars().peekable();
    decode_json_string(&mut chars)
}

/// Extract an unsigned integer field `"key": 123` from within one object
/// substring.
fn extract_number_field(obj: &str, key: &str) -> Option<u64> {
    let after_colon = top_level_value(obj, key)?;
    let digits: String = after_colon.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Parse the `"directories"` array body into [`FrecencyEntry`] values. An
/// object missing its required `"path"` string is skipped rather than
/// aborting the whole parse.
fn parse_directories(array_span: &str) -> Vec<FrecencyEntry> {
    split_top_level_objects(array_span)
        .into_iter()
        .filter_map(|obj| {
            let path = PathBuf::from(extract_string_field(obj, "path")?);
            let visit_count = extract_number_field(obj, "visit_count").unwrap_or(1) as u32;
            let last_visited_ms = extract_number_field(obj, "last_visited_ms").unwrap_or(0);
            Some(FrecencyEntry { path, visit_count, last_visited_ms })
        })
        .collect()
}

// ---------------------------------------------------------------------
// usermenu.toml
// ---------------------------------------------------------------------

/// The name of the F2 user-menu config file, next to `config.toml` in the
/// platform config directory.
pub const USERMENU_FILE: &str = "usermenu.toml";

/// One F2 user-menu entry: a display `label` and the shell `command` it
/// runs, unmodified, when activated (user-menu "Parse label and command
/// entries from usermenu.toml").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMenuEntry {
    pub label: String,
    pub command: String,
}

/// The entries written to a fresh `usermenu.toml` on first run, and used
/// in-memory (without touching the file) when it is malformed (user-menu
/// "Create and recover the usermenu.toml file").
pub fn default_user_menu_entries() -> Vec<UserMenuEntry> {
    vec![
        UserMenuEntry { label: "Open command prompt here".to_string(), command: "cmd.exe".to_string() },
        UserMenuEntry { label: "Directory listing".to_string(), command: "dir".to_string() },
    ]
}

/// The result of parsing `usermenu.toml` content: entries in file order,
/// plus non-fatal warnings (an unknown key on an otherwise-recognized
/// entry, a key found outside any `[[entry]]` block, or an entry missing a
/// field), each naming its line. `malformed` is set when the content
/// contains a line that is neither blank, a comment, an `[[entry]]` header,
/// nor a `key = "value"` pair — real invalid TOML, distinct from a
/// merely-unrecognized key (user-menu "Unknown keys warn rather than fail"
/// vs "Malformed file warns and falls back").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUserMenu {
    pub entries: Vec<UserMenuEntry>,
    pub warnings: Vec<ConfigWarning>,
    pub malformed: bool,
}

/// An `[[entry]]` block being accumulated: the header's line number plus
/// the `label` and `command` seen so far.
struct PendingEntry {
    line: usize,
    label: Option<String>,
    command: Option<String>,
}

/// Parse `usermenu.toml` content: a sequence of `[[entry]]` blocks, each
/// with `label = "..."` and `command = "..."` string keys (basic
/// double-quoted or literal single-quoted TOML strings), read through the
/// shared [`tokenize`] in [`QuoteMode::TomlEscapes`] so a BOM, trailing
/// comments, and `[[entry]] # c` are accepted (design D3). A bare value, a
/// malformed line, or any table other than `[[entry]]` makes the file
/// malformed. Pure and terminal-free, so parsing is unit-testable without
/// touching a file (user-menu "Parsing SHALL be performed by the `config`
/// module ... MUST be unit-testable without a terminal").
pub fn parse_usermenu(input: &str) -> ParsedUserMenu {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut malformed = false;
    let mut current: Option<PendingEntry> = None;
    let mut in_entry = false;

    for line in tokenize(strip_bom(input), QuoteMode::TomlEscapes) {
        let n = line.number;
        match line.kind {
            LineKind::Blank => {}
            LineKind::Table { name, array: true } if name == "entry" => {
                flush_usermenu_entry(&mut current, &mut entries, &mut warnings);
                current = Some(PendingEntry { line: n, label: None, command: None });
                in_entry = true;
            }
            LineKind::Table { .. } | LineKind::Malformed { .. } => malformed = true,
            LineKind::KeyValue { key, value, .. } => {
                let value = match value {
                    Value::Basic(raw) => match unescape_toml_basic(&raw) {
                        Some(v) => v,
                        None => {
                            malformed = true;
                            continue;
                        }
                    },
                    Value::Literal(raw) => raw,
                    // usermenu.toml is real TOML: an unquoted value is a
                    // parse error, not a readability shorthand.
                    Value::Bare(_) => {
                        malformed = true;
                        continue;
                    }
                };
                if !in_entry {
                    warnings.push(ConfigWarning::at(USERMENU_FILE, n, format!("\"{key}\" outside of an [[entry]] block; ignored")));
                    continue;
                }
                let Some(entry) = current.as_mut() else { continue };
                match key.as_str() {
                    "label" => entry.label = Some(value),
                    "command" => entry.command = Some(value),
                    other => warnings.push(ConfigWarning::at(USERMENU_FILE, n, format!("unknown key \"{other}\" in an entry; ignored"))),
                }
            }
        }
    }
    flush_usermenu_entry(&mut current, &mut entries, &mut warnings);

    ParsedUserMenu { entries, warnings, malformed }
}

/// Close out the entry block being accumulated (if any), pushing it as a
/// completed [`UserMenuEntry`] when both `label` and `command` were set, or
/// warning (naming the `[[entry]]` line) and dropping it when either is
/// missing.
fn flush_usermenu_entry(current: &mut Option<PendingEntry>, entries: &mut Vec<UserMenuEntry>, warnings: &mut Vec<ConfigWarning>) {
    let Some(PendingEntry { line, label, command }) = current.take() else { return };
    match (label, command) {
        (Some(label), Some(command)) => entries.push(UserMenuEntry { label, command }),
        (label, _) => {
            let missing = if label.is_none() { "label" } else { "command" };
            warnings.push(ConfigWarning::at(USERMENU_FILE, line, format!("entry missing \"{missing}\"; skipped")));
        }
    }
}

fn unescape_toml_basic(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            other => out.push(other),
        }
    }
    Some(out)
}

/// Render entries back to `usermenu.toml` form (used to write the
/// first-run default file).
pub fn render_usermenu(entries: &[UserMenuEntry]) -> String {
    let mut out = String::new();
    for entry in entries {
        out.push_str("[[entry]]\n");
        out.push_str(&format!("label = {}\n", toml_quote(&entry.label)));
        out.push_str(&format!("command = {}\n", toml_quote(&entry.command)));
        out.push('\n');
    }
    out
}

fn toml_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Why a `usermenu.toml` was treated as malformed (design D3), so the
/// startup warning can say how to recover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserMenuProblem {
    /// The file is UTF-8 text but not valid `usermenu.toml` content.
    InvalidToml,
    /// The file exists but is not UTF-8 text (an ANSI or UTF-16 save).
    NotUtf8,
    /// The file exists but could not be read; carries the OS message.
    Unreadable(String),
}

/// The outcome of loading `usermenu.toml` at startup (user-menu "Create and
/// recover the usermenu.toml file").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMenuLoadResult {
    pub entries: Vec<UserMenuEntry>,
    /// Non-fatal parse warnings, each naming the file and line (empty for a
    /// malformed or freshly created file).
    pub warnings: Vec<ConfigWarning>,
    /// The file did not exist and default entries were written to `path`.
    pub created_default: bool,
    /// The file existed but could not be used; defaults are in effect in
    /// memory and the on-disk file was left untouched — a startup warning
    /// should be shown to the user.
    pub malformed: bool,
    /// Why `malformed` is set; `None` otherwise.
    pub problem: Option<UserMenuProblem>,
}

impl UserMenuLoadResult {
    fn fallback(problem: UserMenuProblem) -> UserMenuLoadResult {
        UserMenuLoadResult {
            entries: default_user_menu_entries(),
            warnings: Vec::new(),
            created_default: false,
            malformed: true,
            problem: Some(problem),
        }
    }
}

/// Load the F2 user menu from `path`. Three outcomes (user-menu "Missing
/// file is created with defaults", "Malformed file warns and falls back
/// without overwriting", well-formed load):
///
/// - Missing file (`NotFound` only): default entries are written to `path`
///   through the durable helper and returned (`created_default: true`).
/// - Malformed content — invalid TOML, a file that is not UTF-8 text
///   (including UTF-16), or any other read error: default entries are
///   returned without touching the file (`malformed: true`, with the
///   reason in `problem`; configuration-files "Non-UTF-8 usermenu.toml is
///   kept, not overwritten").
/// - Well-formed content: the parsed entries (possibly with warnings) are
///   returned, including the zero-entries case (a deliberately empty menu
///   is not malformed).
pub fn load_user_menu(path: &Path) -> UserMenuLoadResult {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let entries = default_user_menu_entries();
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            let _ = crate::durable::replace_file(path, render_usermenu(&entries).as_bytes());
            return UserMenuLoadResult { entries, warnings: Vec::new(), created_default: true, malformed: false, problem: None };
        }
        Err(e) => return UserMenuLoadResult::fallback(UserMenuProblem::Unreadable(e.to_string())),
    };
    let Some(contents) = decode_utf8_text(bytes) else {
        return UserMenuLoadResult::fallback(UserMenuProblem::NotUtf8);
    };
    let parsed = parse_usermenu(&contents);
    if parsed.malformed {
        return UserMenuLoadResult::fallback(UserMenuProblem::InvalidToml);
    }
    UserMenuLoadResult { entries: parsed.entries, warnings: parsed.warnings, created_default: false, malformed: false, problem: None }
}

// ---------------------------------------------------------------------
// Configuration profile directory (config-profile-directory)
// ---------------------------------------------------------------------

/// The three configuration files, in the order the legacy notice lists
/// them (design D3). `"config.toml"` is also the file whose presence in
/// the profile directory silences the notice.
const PROFILE_FILE_NAMES: [&str; 3] = ["config.toml", HISTORY_FILE, USERMENU_FILE];

/// Which of the three configuration files sit in `cwd` while the resolved
/// `profile` directory has no `config.toml` — the pre-profile,
/// per-launch-directory layout (config-profile-directory "Legacy files in
/// the working directory produce a notice, not a migration"; design D3).
/// The result is empty when the profile already has a `config.toml` (the
/// user has moved their settings) or when `cwd` and `profile` are the same
/// directory — portable mode and the last-resort cwd fallback, where those
/// files are the live configuration, not legacy copies. Pure probing over
/// two paths so the notice is unit-testable with temp dirs; nothing here
/// copies, moves, or deletes anything.
pub fn legacy_files_present(cwd: &Path, profile: &Path) -> Vec<&'static str> {
    if cwd == profile || profile.join("config.toml").exists() {
        return Vec::new();
    }
    PROFILE_FILE_NAMES.iter().copied().filter(|name| cwd.join(name).exists()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The settings-only view of [`super::parse`], for the many tests here
    /// that care about the resulting `Config` and not its warnings (this
    /// local definition shadows the glob-imported `parse`). Warning-focused
    /// tests call `super::parse` directly.
    fn parse(input: &str) -> Config {
        super::parse(input).config
    }

    /// The rendered warning lines of a `config.toml` document.
    fn warnings_of(input: &str) -> Vec<String> {
        super::parse(input).warnings.iter().map(ToString::to_string).collect()
    }

    /// A fresh, empty, uniquely named temp directory for a file test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("filecommand-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Every `.fc-tmp` sibling left in `dir` (there must be none after a
    /// completed or failed durable write).
    fn leftover_temp_files(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".fc-tmp") || n.ends_with(".tmp"))
            .collect()
    }

    #[test]
    fn defaults_when_empty() {
        let config = parse("");
        assert_eq!(config, Config::default());
        assert_eq!(config.theme, "nc-classic");
        assert!(config.splash);
        assert_eq!(config.shell, None);
        assert!(config.mouse_enabled, "mouse-input: capture is on by default");
    }

    // -------------------------------------------------------------------
    // [mouse] table (mouse-input "Mouse capture configuration")
    // -------------------------------------------------------------------

    #[test]
    fn mouse_table_disables_capture() {
        let config = parse("[mouse]\nenabled = false\n");
        assert!(!config.mouse_enabled);
    }

    #[test]
    fn mouse_table_explicit_true_stays_enabled() {
        let config = parse("[mouse]\nenabled = true\n");
        assert!(config.mouse_enabled);
    }

    #[test]
    fn bare_top_level_enabled_key_is_not_the_mouse_key() {
        // A bare `enabled = false` with no `[mouse]` header anywhere in the
        // document is an unrecognized top-level key (ignored, like any
        // other) — it must not be mistaken for `[mouse] enabled`.
        let config = parse("enabled = false\ntheme = \"nc-mono\"\n");
        assert!(config.mouse_enabled, "top-level `enabled` outside any table is not the mouse key");
        assert_eq!(config.theme, "nc-mono");
    }

    #[test]
    fn mouse_table_keys_are_scoped_until_the_next_header() {
        // Once a `[mouse]` header is seen, subsequent `key = value` lines
        // belong to that table (only `enabled` is recognized there) until
        // another `[...]` header appears — real TOML section scoping, so a
        // key placed after `[mouse]` is not read as a top-level key.
        let config = parse("[mouse]\nenabled = false\ntheme = \"nc-mono\"\n");
        assert!(!config.mouse_enabled);
        assert_eq!(config.theme, "nc-classic", "theme after the [mouse] header is inside that table, not recognized there");
    }

    #[test]
    fn a_later_mouse_table_overrides_an_earlier_bare_enabled_key() {
        // The bare top-level `enabled` above is never the mouse key, but a
        // `[mouse]` table appearing later in the same document still takes
        // effect normally — table scoping, not a one-shot ignore flag.
        let config = parse("enabled = false\ntheme = \"nc-mono\"\n[mouse]\nenabled = false\n");
        assert!(!config.mouse_enabled);
        assert_eq!(config.theme, "nc-mono", "the top-level theme key, read before the [mouse] header, is unaffected");
    }

    #[test]
    fn mouse_table_tolerates_unknown_keys_and_malformed_value() {
        let config = parse("[mouse]\nfoo = \"bar\"\nenabled = maybe\n");
        assert!(config.mouse_enabled, "an unparsable bool value falls back to the default");
    }

    #[test]
    fn parses_splash_and_theme() {
        let config = parse("splash = false\ntheme = \"nc-mono\"\n");
        assert!(!config.splash);
        assert_eq!(config.theme, "nc-mono");
    }

    #[test]
    fn animations_default_to_on() {
        assert!(parse("").animations, "ui-animations: the default is animated");
        assert!(Config::default().animations);
        assert!(parse("splash = false\n").animations, "unrelated keys leave it on");
    }

    #[test]
    fn animations_can_be_disabled_in_config() {
        assert!(!parse("animations = false\n").animations);
        assert!(parse("animations = true\n").animations);
    }

    #[test]
    fn animations_unparsable_value_falls_back_to_the_default() {
        assert!(parse("animations = maybe\n").animations);
    }

    #[test]
    fn animations_key_inside_the_mouse_table_is_not_a_general_option() {
        // Only `enabled` is read under `[mouse]`; a stray `animations` line
        // there must not flip the general option.
        assert!(parse("[mouse]\nanimations = false\n").animations);
    }

    #[test]
    fn tolerates_comments_and_unknown_keys() {
        let config = parse("# a comment\nfoo = \"bar\"\nsplash=true\n");
        assert!(config.splash);
        assert_eq!(config.theme, "nc-classic");
    }

    #[test]
    fn tolerates_malformed_lines() {
        let config = parse("this is not valid toml at all\nsplash = true\n");
        assert!(config.splash);
    }

    #[test]
    fn load_missing_file_returns_default() {
        let parsed = load(Path::new("this/path/definitely/does/not/exist/config.toml"));
        assert_eq!(parsed.config, Config::default());
        assert!(parsed.warnings.is_empty(), "a missing config.toml is not a warning");
    }

    #[test]
    fn parses_shell_quoted_and_bare() {
        assert_eq!(parse("shell = \"powershell\"\n").shell.as_deref(), Some("powershell"));
        assert_eq!(parse("shell = pwsh -NoLogo -Command\n").shell.as_deref(), Some("pwsh -NoLogo -Command"));
        assert_eq!(parse("shell = \n").shell, None);
    }

    #[test]
    fn parses_editor_quoted_and_bare_and_treats_blank_as_unset() {
        assert_eq!(parse("editor = \"notepad\"\n").editor.as_deref(), Some("notepad"));
        assert_eq!(parse("editor = code --wait\n").editor.as_deref(), Some("code --wait"));
        assert_eq!(parse("editor = \n").editor, None);
        assert_eq!(parse("editor = \"\"\n").editor, None);
        assert_eq!(parse("").editor, None);
    }

    #[test]
    fn parses_overridable_paste_bindings() {
        let config = parse("key.paste_name = \"alt+n\"\nkey.paste_path = \"ctrl+shift+p\"\n");
        assert_eq!(config.keys.paste_name, KeyBinding::new(false, true, false, "n"));
        assert_eq!(config.keys.paste_path, KeyBinding::new(true, false, true, "p"));
    }

    #[test]
    fn parses_overridable_quick_filter_and_fuzzy_jump_bindings() {
        let config = parse("key.quick_filter = \"alt+p\"\nkey.fuzzy_jump = \"ctrl+shift+j\"\n");
        assert_eq!(config.keys.quick_filter, KeyBinding::new(false, true, false, "p"));
        assert_eq!(config.keys.fuzzy_jump, KeyBinding::new(true, false, true, "j"));
    }

    #[test]
    fn default_bindings_are_ctrl_enter_and_ctrl_bracket() {
        let keys = Keys::default();
        assert_eq!(keys.paste_name, KeyBinding::new(true, false, false, "enter"));
        assert_eq!(keys.paste_path, KeyBinding::new(true, false, false, "]"));
        assert_eq!(keys.quick_filter, KeyBinding::new(true, false, false, "p"));
        assert_eq!(keys.fuzzy_jump, KeyBinding::new(true, false, false, "j"));
        assert_eq!(keys.split_left, KeyBinding::new(true, false, false, "left"));
        assert_eq!(keys.split_right, KeyBinding::new(true, false, false, "right"));
        assert_eq!(keys.split_reset, KeyBinding::new(true, false, false, "="));
        // clipboard-export "Clipboard key bindings": Ctrl+C for Files,
        // Ctrl+Shift+Ins for Paths.
        assert_eq!(keys.clipboard_files, KeyBinding::new(true, false, false, "c"));
        assert_eq!(keys.clipboard_paths, KeyBinding::new(true, false, true, "insert"));
        // command-line "Clear the command line with a dedicated key":
        // Ctrl+Y by default.
        assert_eq!(keys.clear_line, KeyBinding::new(true, false, false, "y"));
    }

    #[test]
    fn parses_overridable_clear_line_binding() {
        // command-line "Clear the command line with a dedicated key"
        // scenario "Rebinding the clear key".
        let config = parse("key.clear_line = \"ctrl+k\"\n");
        assert_eq!(config.keys.clear_line, KeyBinding::new(true, false, false, "k"));
        assert_eq!(parse("").keys.clear_line, KeyBinding::new(true, false, false, "y"), "unset keeps the default");
    }

    #[test]
    fn parses_overridable_clipboard_bindings() {
        // clipboard-export "Rebinding the files chord": `key.clipboard_files
        // = "ctrl+k"` rebinds Files; Paths is independently overridable too.
        let config = parse("key.clipboard_files = \"ctrl+k\"\nkey.clipboard_paths = \"alt+shift+c\"\n");
        assert_eq!(config.keys.clipboard_files, KeyBinding::new(true, false, false, "k"));
        assert_eq!(config.keys.clipboard_paths, KeyBinding::new(false, true, true, "c"));
    }

    #[test]
    fn parses_overridable_split_bindings() {
        // panel-split "Adjust and reset the panel split": "All three
        // bindings SHALL be overridable in config.toml".
        let config = parse("key.split_left = \"alt+left\"\nkey.split_right = \"alt+right\"\nkey.split_reset = \"ctrl+shift+equal\"\n");
        assert_eq!(config.keys.split_left, KeyBinding::new(false, true, false, "left"));
        assert_eq!(config.keys.split_right, KeyBinding::new(false, true, false, "right"));
        assert_eq!(config.keys.split_reset, KeyBinding::new(true, false, true, "equal"));
    }

    #[test]
    fn parse_binding_handles_modifiers_and_bare_keys() {
        assert_eq!(parse_binding("ctrl+enter"), Some(KeyBinding::new(true, false, false, "enter")));
        assert_eq!(parse_binding("F9"), Some(KeyBinding::new(false, false, false, "f9")));
        assert_eq!(parse_binding("ctrl++"), Some(KeyBinding::new(true, false, false, "+")));
        assert_eq!(parse_binding("ctrl"), None);
    }

    // -------------------------------------------------------------------
    // Shared tokenizer (configuration-files "Configuration files tolerate a
    // UTF-8 BOM and inline comments"; design D1)
    // -------------------------------------------------------------------

    fn kv(key: &str, value: Value, comment: Option<&str>) -> LineKind {
        LineKind::KeyValue { key: key.to_string(), value, comment: comment.map(str::to_string) }
    }

    fn one(input: &str, mode: QuoteMode) -> LineKind {
        tokenize_line(input, mode)
    }

    #[test]
    fn strip_bom_removes_exactly_one_leading_mark() {
        assert_eq!(strip_bom("\u{feff}splash = false"), "splash = false");
        assert_eq!(strip_bom("splash = false"), "splash = false");
        assert_eq!(strip_bom("\u{feff}\u{feff}x"), "\u{feff}x");
        assert_eq!(strip_bom(""), "");
    }

    #[test]
    fn tokenizer_blank_lines_and_whole_line_comments() {
        assert_eq!(one("", QuoteMode::Verbatim), LineKind::Blank);
        assert_eq!(one("   ", QuoteMode::Verbatim), LineKind::Blank);
        assert_eq!(one("# a comment", QuoteMode::Verbatim), LineKind::Blank);
        assert_eq!(one("   # indented comment", QuoteMode::Verbatim), LineKind::Blank);
    }

    #[test]
    fn tokenizer_table_headers_with_and_without_comments() {
        assert_eq!(one("[mouse]", QuoteMode::Verbatim), LineKind::Table { name: "mouse".to_string(), array: false });
        assert_eq!(one("[mouse] # pointer settings", QuoteMode::Verbatim), LineKind::Table { name: "mouse".to_string(), array: false });
        assert_eq!(one("[[entry]]", QuoteMode::TomlEscapes), LineKind::Table { name: "entry".to_string(), array: true });
        assert_eq!(one("[[entry]] # c", QuoteMode::TomlEscapes), LineKind::Table { name: "entry".to_string(), array: true });
        assert!(matches!(one("[mouse", QuoteMode::Verbatim), LineKind::Malformed { .. }));
        assert!(matches!(one("[mouse] x", QuoteMode::Verbatim), LineKind::Malformed { .. }));
    }

    #[test]
    fn tokenizer_quoted_values_keep_hashes_and_drop_comments() {
        assert_eq!(one("k = \"a # b\"", QuoteMode::Verbatim), kv("k", Value::Basic("a # b".to_string()), None));
        assert_eq!(one("k = \"x\" # c \"d\"", QuoteMode::Verbatim), kv("k", Value::Basic("x".to_string()), Some("# c \"d\"")));
        assert_eq!(one("k = 'x' # c", QuoteMode::Verbatim), kv("k", Value::Literal("x".to_string()), Some("# c")));
        assert_eq!(
            one("k = \"C:\\dir\\\"", QuoteMode::Verbatim),
            kv("k", Value::Basic("C:\\dir\\".to_string()), None),
            "a trailing backslash still terminates in Verbatim mode"
        );
        assert_eq!(
            one("k = \"say \\\"hi\\\"\"", QuoteMode::Verbatim),
            kv("k", Value::Basic("say \\\"hi\\\"".to_string()), None),
            "verbatim inner text"
        );
        assert_eq!(one("k = \"a\" b\"", QuoteMode::Verbatim), kv("k", Value::Basic("a\" b".to_string()), None), "the last quote qualifies");
        assert_eq!(one("k = \"\"", QuoteMode::Verbatim), kv("k", Value::Basic(String::new()), None));
    }

    #[test]
    fn tokenizer_toml_escapes_mode_does_not_close_on_an_escaped_quote() {
        assert_eq!(one("k = \"a \\\" # b\" # c", QuoteMode::TomlEscapes), kv("k", Value::Basic("a \\\" # b".to_string()), Some("# c")));
        assert!(
            matches!(one("k = \"C:\\dir\\\"", QuoteMode::TomlEscapes), LineKind::Malformed { .. }),
            "in TOML the backslash escapes the quote, so it is unterminated"
        );
    }

    #[test]
    fn tokenizer_bare_values_start_a_comment_only_after_whitespace() {
        assert_eq!(one("k = C:\\Tools\\C#\\edit.exe", QuoteMode::Verbatim), kv("k", Value::Bare("C:\\Tools\\C#\\edit.exe".to_string()), None));
        assert_eq!(one("k = v # c", QuoteMode::Verbatim), kv("k", Value::Bare("v".to_string()), Some("# c")));
        assert_eq!(one("k = #c", QuoteMode::Verbatim), kv("k", Value::Bare(String::new()), Some("#c")));
        assert_eq!(one("k = true", QuoteMode::Verbatim), kv("k", Value::Bare("true".to_string()), None));
        assert_eq!(one("k =", QuoteMode::Verbatim), kv("k", Value::Bare(String::new()), None));
        assert_eq!(one("k=v", QuoteMode::Verbatim), kv("k", Value::Bare("v".to_string()), None));
    }

    #[test]
    fn tokenizer_malformed_lines() {
        assert_eq!(one("k = \"x", QuoteMode::Verbatim), LineKind::Malformed { reason: "unterminated quoted value".to_string() });
        assert_eq!(one("k = \"x\" y", QuoteMode::Verbatim), LineKind::Malformed { reason: "unexpected text after the closing quote".to_string() });
        assert_eq!(one("k \"x\"", QuoteMode::Verbatim), LineKind::Malformed { reason: "expected key = value".to_string() });
        assert!(matches!(one("= x", QuoteMode::Verbatim), LineKind::Malformed { .. }));
    }

    #[test]
    fn tokenizer_line_numbers_are_one_based_and_survive_crlf() {
        let lines = tokenize("# c\r\n\r\nsplash = false\r\n[mouse]\r\nenabled = true", QuoteMode::Verbatim);
        let numbers: Vec<usize> = lines.iter().map(|l| l.number).collect();
        assert_eq!(numbers, vec![1, 2, 3, 4, 5]);
        assert_eq!(lines[2].kind, kv("splash", Value::Bare("false".to_string()), None));
        assert_eq!(lines[3].kind, LineKind::Table { name: "mouse".to_string(), array: false });
        assert_eq!(lines[4].kind, kv("enabled", Value::Bare("true".to_string()), None));
    }

    // -------------------------------------------------------------------
    // config.toml: comments, BOM, and warnings (configuration-files
    // "Malformed or unrecognized configuration lines produce startup
    // warnings"; design D2)
    // -------------------------------------------------------------------

    /// Render a `Config` as a complete `config.toml`, every recognized key
    /// present, optionally with a comment after each value. Quoted values
    /// are verbatim (`config.toml` has no escapes), so `vq`, not
    /// `toml_quote`.
    fn render_full_config(config: &Config, comment: &str) -> String {
        let vq = |s: &str| format!("\"{s}\"");
        let b = |k: &KeyBinding| {
            let mut parts = Vec::new();
            if k.ctrl {
                parts.push("ctrl");
            }
            if k.alt {
                parts.push("alt");
            }
            if k.shift {
                parts.push("shift");
            }
            parts.push(&k.key);
            vq(&parts.join("+"))
        };
        let keys = &config.keys;
        let lines = [
            format!("splash = {}", config.splash),
            format!("animations = {}", config.animations),
            format!("theme = {}", vq(&config.theme)),
            format!("shell = {}", vq(config.shell.as_deref().unwrap_or("cmd.exe /C"))),
            format!("editor = {}", vq(config.editor.as_deref().unwrap_or("code -w"))),
            format!("panel_split = {}", config.panel_split),
            format!("key.paste_name = {}", b(&keys.paste_name)),
            format!("key.paste_path = {}", b(&keys.paste_path)),
            format!("key.quick_filter = {}", b(&keys.quick_filter)),
            format!("key.fuzzy_jump = {}", b(&keys.fuzzy_jump)),
            format!("key.split_left = {}", b(&keys.split_left)),
            format!("key.split_right = {}", b(&keys.split_right)),
            format!("key.split_reset = {}", b(&keys.split_reset)),
            format!("key.clipboard_files = {}", b(&keys.clipboard_files)),
            format!("key.clipboard_paths = {}", b(&keys.clipboard_paths)),
            format!("key.clear_line = {}", b(&keys.clear_line)),
            "[mouse]".to_string(),
            format!("enabled = {}", config.mouse_enabled),
        ];
        lines.iter().map(|l| format!("{l}{comment}\n")).collect()
    }

    #[test]
    fn default_config_rendered_as_a_file_parses_back_with_zero_warnings() {
        let rendered = render_full_config(&Config::default(), "");
        let parsed = super::parse(&rendered);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        let expected = Config { shell: Some("cmd.exe /C".to_string()), editor: Some("code -w".to_string()), ..Config::default() };
        assert_eq!(parsed.config, expected);
    }

    /// R-16 regression: an inline comment after every recognized key's
    /// value yields the same `Config` as the uncommented file.
    #[test]
    fn inline_comments_after_every_recognized_key_do_not_change_the_config() {
        let custom = Config {
            splash: false,
            animations: false,
            theme: "purple-lights".to_string(),
            shell: Some("pwsh -NoLogo -Command".to_string()),
            editor: Some("C:\\Tools\\C# Editor\\edit.exe".to_string()),
            panel_split: 40,
            mouse_enabled: false,
            keys: Keys { fuzzy_jump: KeyBinding::new(true, false, true, "g"), ..Keys::default() },
        };
        let plain = super::parse(&render_full_config(&custom, ""));
        let commented = super::parse(&render_full_config(&custom, " # a comment"));
        assert!(plain.warnings.is_empty(), "{:?}", plain.warnings);
        assert!(commented.warnings.is_empty(), "{:?}", commented.warnings);
        assert_eq!(plain.config, custom);
        assert_eq!(commented.config, plain.config);
    }

    #[test]
    fn bom_and_inline_comments_are_read_as_intended() {
        let p = super::parse("\u{feff}splash = false # start straight in the panels\ntheme = \"purple-lights\" # favourite\npanel_split = 40 # c\n");
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        assert!(!p.config.splash);
        assert_eq!(p.config.theme, "purple-lights");
        assert_eq!(p.config.panel_split, 40);
        assert_eq!(parse("editor = \"C:\\Tools\\C# Editor\\edit.exe\"\n").editor.as_deref(), Some("C:\\Tools\\C# Editor\\edit.exe"));
        assert_eq!(parse("editor = C:\\Tools\\C#\\edit.exe\n").editor.as_deref(), Some("C:\\Tools\\C#\\edit.exe"));
        assert!(!parse("[mouse] # pointer settings\nenabled = false\n").mouse_enabled);
    }

    /// The README example as it stood before this change: `[mouse]` before
    /// the ten `key.*` lines.
    const OLD_README_EXAMPLE: &str = "splash = true\nanimations = true\ntheme = \"nc-classic\"\nshell = \"cmd.exe /C\"\neditor = \"code -w\"\npanel_split = 50\n\n[mouse]\nenabled = true\n\nkey.paste_name = \"ctrl+enter\"\nkey.paste_path = \"ctrl+]\"\nkey.quick_filter = \"ctrl+p\"\nkey.fuzzy_jump = \"ctrl+j\"\nkey.split_left = \"ctrl+left\"\nkey.split_right = \"ctrl+right\"\nkey.split_reset = \"ctrl+=\"\nkey.clipboard_files = \"ctrl+c\"\nkey.clipboard_paths = \"ctrl+shift+insert\"\nkey.clear_line = \"ctrl+y\"\n";

    /// R-16 regression: the old README example silently dropped every
    /// binding. Now each misplaced `key.*` line is reported by number.
    #[test]
    fn readme_example_with_mouse_table_first_warns_for_every_misplaced_binding() {
        let parsed = super::parse(OLD_README_EXAMPLE);
        assert_eq!(parsed.config.keys, Keys::default(), "the bindings stay at their defaults, as before");
        assert_eq!(parsed.warnings.len(), 10, "{:?}", parsed.warnings);
        for (i, w) in parsed.warnings.iter().enumerate() {
            assert_eq!(w.file, CONFIG_FILE);
            assert_eq!(w.line, 11 + i, "{w}");
            assert!(w.message.contains("not a [mouse] setting"), "{w}");
            assert!(w.message.contains("before the first [table]"), "{w}");
        }
        assert_eq!(
            parsed.warnings[3].to_string(),
            "config.toml line 14: \"key.fuzzy_jump\" is not a [mouse] setting and was ignored — put top-level settings before the first [table]"
        );
        assert_eq!(summarize_warnings(&parsed.warnings).unwrap(), format!("{} (+9 more)", parsed.warnings[0]));
    }

    #[test]
    fn corrected_readme_example_parses_with_zero_warnings_and_all_bindings() {
        // `[mouse]` moved to the end, and `key.fuzzy_jump` changed so the
        // bindings demonstrably apply.
        let corrected =
            OLD_README_EXAMPLE.replace("[mouse]\nenabled = true\n\n", "").replace("key.fuzzy_jump = \"ctrl+j\"", "key.fuzzy_jump = \"ctrl+g\"")
                + "\n[mouse]\nenabled = false\n";
        let parsed = super::parse(&corrected);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.config.keys.fuzzy_jump, KeyBinding::new(true, false, false, "g"));
        assert!(!parsed.config.mouse_enabled);
        let expected_keys = Keys { fuzzy_jump: KeyBinding::new(true, false, false, "g"), ..Keys::default() };
        assert_eq!(parsed.config.keys, expected_keys, "all ten bindings applied");
    }

    #[test]
    fn warnings_for_unknown_key_malformed_line_unknown_table_and_bad_values() {
        assert_eq!(warnings_of("colour = \"red\"\nsplash = false\n"), vec!["config.toml line 1: unknown setting \"colour\"; ignored"]);
        assert_eq!(warnings_of("# c\nsplash false\n"), vec!["config.toml line 2: expected key = value; line ignored"]);
        assert!(super::parse("# c\nsplash false\n").config.splash, "the line is ignored and the default stands");
        assert_eq!(
            warnings_of("[colors]\nfg = \"red\"\nbg = \"blue\"\n"),
            vec!["config.toml line 1: unknown table [colors]; its settings are ignored"],
            "one warning for the header covers its keys"
        );
        assert_eq!(warnings_of("[[entry]]\n"), vec!["config.toml line 1: unknown table [[entry]]; its settings are ignored"]);
        let p = super::parse("animations = yes\n");
        assert!(p.config.animations, "the default stands");
        assert_eq!(p.warnings[0].to_string(), "config.toml line 1: animations must be true or false; using the default");
        assert_eq!(
            warnings_of("splash = \"true\"\n"),
            vec!["config.toml line 1: splash must be true or false; using the default"],
            "booleans are bare in TOML"
        );
        assert_eq!(warnings_of("[mouse]\nenabled = maybe\n"), vec!["config.toml line 2: enabled must be true or false; using the default"]);
        let p = super::parse("key.fuzzy_jump = \"\"\n");
        assert_eq!(p.config.keys.fuzzy_jump, Keys::default().fuzzy_jump);
        assert_eq!(p.warnings[0].to_string(), "config.toml line 1: key.fuzzy_jump \"\" is not a key binding; using the default");
        assert_eq!(
            warnings_of("key.paste_name = \"ctrl\"\n"),
            vec!["config.toml line 1: key.paste_name \"ctrl\" is not a key binding; using the default"]
        );
        assert_eq!(warnings_of("key.paste_name = \"ctrl+\"\n"), Vec::<String>::new(), "a trailing + is the + key, as parse_binding always read it");
    }

    #[test]
    fn theme_and_panel_split_values_keep_their_silent_fallbacks() {
        assert_eq!(warnings_of("theme = \"no-such-theme\"\n"), Vec::<String>::new());
        assert_eq!(warnings_of("panel_split = \"wide\"\npanel_split = 150\n"), Vec::<String>::new());
        let p = super::parse("# c\n# c\ntheme = \"purple-lights\n");
        assert_eq!(p.config.theme, DEFAULT_THEME_NAME, "the unterminated line is ignored");
        assert_eq!(
            p.warnings.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["config.toml line 3: unterminated quoted value; line ignored"]
        );
    }

    #[test]
    fn summarize_warnings_formats_first_plus_count() {
        let none: Vec<String> = Vec::new();
        assert_eq!(summarize_warnings(&none), None);
        assert_eq!(summarize_warnings(&["a".to_string()]).as_deref(), Some("a"));
        assert_eq!(summarize_warnings(&["a".to_string(), "b".to_string(), "c".to_string()]).as_deref(), Some("a (+2 more)"));
        let w = ConfigWarning::whole_file(CONFIG_FILE, "is not UTF-8 text; using default settings");
        assert_eq!(w.to_string(), "config.toml is not UTF-8 text; using default settings");
    }

    /// `load`: missing → defaults silently; Windows-1252 → defaults plus the
    /// not-UTF-8 warning; BOM → the first key is read.
    #[test]
    fn load_distinguishes_missing_not_utf8_and_bom_files() {
        let dir = temp_dir("load");
        let path = dir.join(CONFIG_FILE);

        let missing = load(&path);
        assert_eq!(missing, ParsedConfig::default());

        std::fs::write(&path, b"theme = \"caf\xe9\"\nsplash = false\n").unwrap();
        let ansi = load(&path);
        assert_eq!(ansi.config, Config::default(), "defaults apply");
        assert_eq!(ansi.warnings.iter().map(ToString::to_string).collect::<Vec<_>>(), vec!["config.toml is not UTF-8 text; using default settings"]);

        std::fs::write(&path, "\u{feff}splash = false\n").unwrap();
        let bom = load(&path);
        assert!(!bom.config.splash, "the first key is read despite the BOM");
        assert!(bom.warnings.is_empty(), "{:?}", bom.warnings);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // -------------------------------------------------------------------
    // config.toml theme persistence (visual-themes)
    // -------------------------------------------------------------------

    #[test]
    fn set_theme_line_replaces_an_existing_theme_key_leaving_other_lines_untouched() {
        let input = "splash = true\ntheme = \"nc-classic\"\nshell = \"cmd.exe /C\"\n";
        let updated = set_theme_line(input, "purple-lights");
        assert_eq!(updated, "splash = true\ntheme = \"purple-lights\"\nshell = \"cmd.exe /C\"\n");
    }

    #[test]
    fn set_theme_line_appends_when_no_theme_key_is_present() {
        let input = "splash = true\n";
        let updated = set_theme_line(input, "yellow-storm");
        assert_eq!(updated, "splash = true\ntheme = \"yellow-storm\"\n");
    }

    #[test]
    fn set_theme_line_on_empty_input_yields_just_the_theme_key() {
        assert_eq!(set_theme_line("", "inverted"), "theme = \"inverted\"\n");
    }

    #[test]
    fn set_theme_line_preserves_comments_and_a_commented_out_theme_line() {
        let input = "# a comment\n# theme = \"ignored\"\ntheme = \"nc-mono\"\n";
        let updated = set_theme_line(input, "terminal-green");
        assert_eq!(updated, "# a comment\n# theme = \"ignored\"\ntheme = \"terminal-green\"\n");
    }

    #[test]
    fn save_theme_atomic_writes_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("filecommand-config-theme-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "splash = false\ntheme = \"nc-classic\"\n").unwrap();

        save_theme_atomic(&path, "purple-lights").expect("write theme");
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "splash = false\ntheme = \"purple-lights\"\n");
        assert_eq!(leftover_temp_files(&dir), Vec::<String>::new(), "temp file must be renamed away");

        let loaded = load(&path).config;
        assert_eq!(loaded.theme, "purple-lights");
        assert!(!loaded.splash, "unrelated keys survive the theme write");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_theme_atomic_creates_config_toml_when_missing() {
        let dir = std::env::temp_dir().join(format!("filecommand-config-theme-test-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");

        save_theme_atomic(&path, "inverted").expect("write theme to a fresh file");
        assert_eq!(load(&path).config.theme, "inverted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// configuration-files "Failed configuration write leaves the file
    /// untouched": when the replace step fails (here: another handle holds
    /// `config.toml` open with sharing denied, so the rename over it is
    /// refused), the target is byte-identical and no temp file remains.
    #[cfg(windows)]
    #[test]
    fn save_theme_atomic_failure_leaves_config_untouched_and_no_temp_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp_dir("theme-write-fails");
        let path = dir.join("config.toml");
        let original = "splash = false\ntheme = \"nc-classic\" # keep\n";
        std::fs::write(&path, original).unwrap();
        let lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).expect("open config.toml with sharing denied");

        let err = save_theme_atomic(&path, "purple-lights").expect_err("the replace must fail while the target is locked");
        assert_ne!(err.kind(), io::ErrorKind::NotFound);
        drop(lock);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original, "the target is unchanged");
        assert_eq!(leftover_temp_files(&dir), Vec::<String>::new(), "the temp file is removed on failure");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `config.toml` that is not UTF-8 text is refused rather than
    /// replaced by a one-line file: the rewrite could not keep its other
    /// settings.
    #[test]
    fn save_theme_atomic_refuses_to_clobber_a_non_utf8_config() {
        let dir = temp_dir("theme-write-ansi");
        let path = dir.join("config.toml");
        let ansi = b"theme = \"caf\xe9\"\n".to_vec();
        std::fs::write(&path, &ansi).unwrap();
        save_theme_atomic(&path, "inverted").expect_err("non-UTF-8 config.toml is not rewritten");
        assert_eq!(std::fs::read(&path).unwrap(), ansi);
        assert_eq!(leftover_temp_files(&dir), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_panel_split() {
        let config = parse("panel_split = 66\n");
        assert_eq!(config.panel_split, 66);
    }

    #[test]
    fn invalid_panel_split_falls_back_to_fifty() {
        // panel-split "Invalid configured value falls back".
        assert_eq!(parse("panel_split = \"wide\"\n").panel_split, 50);
        assert_eq!(parse("panel_split = 150\n").panel_split, 50);
        assert_eq!(parse("splash = true\n").panel_split, 50, "unset falls back too");
    }

    #[test]
    fn set_panel_split_line_replaces_an_existing_key_leaving_other_lines_untouched() {
        let input = "splash = true\npanel_split = 50\nshell = \"cmd.exe /C\"\n";
        let updated = set_panel_split_line(input, 66);
        assert_eq!(updated, "splash = true\npanel_split = 66\nshell = \"cmd.exe /C\"\n");
    }

    #[test]
    fn set_panel_split_line_appends_when_no_key_is_present() {
        let input = "splash = true\n";
        let updated = set_panel_split_line(input, 40);
        assert_eq!(updated, "splash = true\npanel_split = 40\n");
    }

    #[test]
    fn save_panel_split_atomic_writes_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("filecommand-config-panel-split-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "splash = false\npanel_split = 50\n").unwrap();

        save_panel_split_atomic(&path, 66).expect("write panel_split");
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "splash = false\npanel_split = 66\n");
        assert_eq!(leftover_temp_files(&dir), Vec::<String>::new(), "temp file must be renamed away");

        let loaded = load(&path).config;
        assert_eq!(loaded.panel_split, 66);
        assert!(!loaded.splash, "unrelated keys survive the write");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_panel_split_atomic_creates_config_toml_when_missing() {
        let dir = std::env::temp_dir().join(format!("filecommand-config-panel-split-test-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");

        save_panel_split_atomic(&path, 40).expect("write panel_split to a fresh file");
        assert_eq!(load(&path).config.panel_split, 40);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -------------------------------------------------------------------
    // Table-aware rewrites (configuration-files "Configuration rewrites
    // keep settings in the top-level table"; design D5)
    // -------------------------------------------------------------------

    #[test]
    fn set_top_level_key_inserts_a_missing_key_above_the_first_table() {
        let input = "[mouse]\nenabled = false\n";
        let out = set_theme_line(input, "purple-lights");
        assert_eq!(out, "theme = \"purple-lights\"\n[mouse]\nenabled = false\n");
        let parsed = super::parse(&out);
        assert_eq!(parsed.config.theme, "purple-lights");
        assert!(!parsed.config.mouse_enabled, "the [mouse] table still applies");
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);

        let out = set_panel_split_line(input, 60);
        assert_eq!(out, "panel_split = 60\n[mouse]\nenabled = false\n");
        assert_eq!(super::parse(&out).config.panel_split, 60);
    }

    #[test]
    fn set_top_level_key_replaces_keeping_the_trailing_comment() {
        let input = "# header\nsplash = true\ntheme = \"nc-classic\" # my pick\nshell = \"cmd.exe /C\"\n";
        let out = set_theme_line(input, "yellow-storm");
        assert_eq!(out, "# header\nsplash = true\ntheme = \"yellow-storm\" # my pick\nshell = \"cmd.exe /C\"\n");
        let parsed = super::parse(&out);
        assert_eq!(parsed.config.theme, "yellow-storm");
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    }

    #[test]
    fn set_top_level_key_preserves_the_bom_with_exactly_one_key_line() {
        let input = "\u{feff}theme = \"nc-classic\"\nsplash = true\n";
        let out = set_theme_line(input, "inverted");
        assert_eq!(out, "\u{feff}theme = \"inverted\"\nsplash = true\n");
        assert_eq!(out.matches("theme =").count(), 1);
        assert_eq!(super::parse(&out).config.theme, "inverted");
    }

    #[test]
    fn set_top_level_key_does_not_replace_a_key_inside_a_table() {
        let input = "splash = true\n[mouse]\ntheme = \"nc-mono\"\n";
        let out = set_theme_line(input, "inverted");
        assert_eq!(
            out, "splash = true\ntheme = \"inverted\"\n[mouse]\ntheme = \"nc-mono\"\n",
            "the in-table line is left as it was; the new line goes above the table"
        );
        assert_eq!(super::parse(&out).config.theme, "inverted");
    }

    #[test]
    fn set_top_level_key_appends_when_there_is_no_table_and_keeps_crlf() {
        assert_eq!(set_theme_line("splash = true\n", "x"), "splash = true\ntheme = \"x\"\n");
        assert_eq!(set_theme_line("splash = true\r\n# c\r\n", "x"), "splash = true\r\n# c\r\ntheme = \"x\"\r\n", "CRLF input stays CRLF");
        assert_eq!(set_theme_line("splash = true\r\ntheme = \"a\" # k\r\n", "b"), "splash = true\r\ntheme = \"b\" # k\r\n");
    }

    #[test]
    fn set_top_level_key_round_trips_with_no_warnings() {
        let input = "# comment\nsplash = false # inline\neditor = C:\\Tools\\C#\\edit.exe\n\n[mouse] # pointer\nenabled = false\n";
        let out = set_panel_split_line(input, 35);
        let parsed = super::parse(&out);
        assert_eq!(parsed.config.panel_split, 35);
        assert_eq!(parsed.config.editor.as_deref(), Some("C:\\Tools\\C#\\edit.exe"));
        assert!(!parsed.config.splash);
        assert!(!parsed.config.mouse_enabled);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        // Every original line is still present, byte for byte.
        for line in input.lines() {
            assert!(out.lines().any(|l| l == line), "line {line:?} missing from {out:?}");
        }
    }

    #[test]
    fn history_json_roundtrips_including_escapes() {
        let entries = vec![r#"echo "hi""#.to_string(), "dir C:\\Users".to_string(), "line\nbreak".to_string()];
        let rendered = render_history(&entries);
        assert_eq!(parse_history(&rendered), entries);
    }

    #[test]
    fn empty_history_renders_as_empty_array() {
        assert_eq!(render_history(&[]), "[]\n");
        assert!(parse_history("[]\n").is_empty());
    }

    #[test]
    fn parse_history_tolerates_truncated_input() {
        assert_eq!(parse_history("[\"complete\", \"trunc"), vec!["complete".to_string()]);
    }

    // -------------------------------------------------------------------
    // Lossless history string decoding (configuration-files "History file
    // strings decode without losing entries"; design D7)
    // -------------------------------------------------------------------

    #[test]
    fn history_surrogate_pair_decodes_to_one_character() {
        assert_eq!(parse_history(r#"["echo \uD83D\uDE00"]"#), vec!["echo 😀".to_string()]);
    }

    #[test]
    fn history_lone_or_bad_unicode_escapes_become_replacement_chars_without_dropping_entries() {
        assert_eq!(parse_history(r#"["cd a","cd b\uD83D","cd c"]"#), vec!["cd a".to_string(), "cd b\u{fffd}".to_string(), "cd c".to_string()]);
        assert_eq!(
            parse_history(r#"["x\uZZZZy","z"]"#),
            vec!["x\u{fffd}ZZZZy".to_string(), "z".to_string()],
            "a \\u without hex digits is U+FFFD and parsing continues"
        );
        assert_eq!(parse_history(r#"["a\uDE00b"]"#), vec!["a\u{fffd}b".to_string()], "a stray low surrogate");
        assert_eq!(
            parse_history(r#"["a\uD83Db","c"]"#),
            vec!["a\u{fffd}b".to_string(), "c".to_string()],
            "a high surrogate not followed by a low one"
        );
        assert_eq!(parse_history(r#"["a\u00e9\u0041\n\t\b\f\/\\\""]"#), vec!["aéA\n\t\u{8}\u{c}/\\\"".to_string()]);
        assert_eq!(parse_history(r#"["short\u12"]"#), vec!["short\u{fffd}12".to_string()], "a short escape never swallows the closing quote");
    }

    #[test]
    fn history_file_key_text_inside_a_string_is_not_mistaken_for_the_key() {
        let input = "{\n  \"commands\": [\"directories\", \"cd C:\\\\\"],\n  \"directories\": [\n    {\"path\": \"C:\\\\a\", \"visit_count\": 2, \"last_visited_ms\": 5},\n    {\"path\": \"C:\\\\b\", \"visit_count\": 1, \"last_visited_ms\": 6}\n  ]\n}\n";
        let parsed = parse_history_file(input);
        assert_eq!(parsed.commands, vec!["directories".to_string(), "cd C:\\".to_string()]);
        assert_eq!(parsed.directories.len(), 2, "{:?}", parsed.directories);
        assert_eq!(parsed.directories[0].path, PathBuf::from("C:\\a"));
        assert_eq!(parsed.directories[1].path, PathBuf::from("C:\\b"));
        // And a directory object whose path *contains* the field name text.
        let tricky = "{\"commands\": [], \"directories\": [{\"visit_count\": 3, \"path\": \"C:\\\\\\\"path\\\"\", \"last_visited_ms\": 1}]}";
        let parsed = parse_history_file(tricky);
        assert_eq!(parsed.directories, vec![FrecencyEntry { path: PathBuf::from("C:\\\"path\""), visit_count: 3, last_visited_ms: 1 }]);
    }

    #[test]
    fn history_directory_with_a_surrogate_escape_in_its_path_is_kept() {
        let input = "{\"commands\": [], \"directories\": [{\"path\": \"C:\\\\\\uD83D\", \"visit_count\": 1, \"last_visited_ms\": 1}, {\"path\": \"C:\\\\ok\", \"visit_count\": 1, \"last_visited_ms\": 2}]}";
        let parsed = parse_history_file(input);
        assert_eq!(parsed.directories.len(), 2);
        assert_eq!(parsed.directories[0].path, PathBuf::from("C:\\\u{fffd}"));
    }

    #[test]
    fn bom_prefixed_history_file_parses() {
        let file = sample_history_file();
        let rendered = format!("\u{feff}{}", render_history_file(&file));
        assert_eq!(parse_history_file(&rendered), file);
        assert_eq!(parse_history("\u{feff}[\"a\"]"), vec!["a".to_string()]);
    }

    #[test]
    fn push_history_dedupes_and_caps() {
        let mut history = vec!["a".to_string(), "b".to_string()];
        push_history(&mut history, "a");
        assert_eq!(history, vec!["b".to_string(), "a".to_string()]);
        push_history(&mut history, "   ");
        assert_eq!(history.len(), 2, "blank commands are not recorded");

        let mut history: Vec<String> = (0..MAX_HISTORY).map(|i| format!("cmd{i}")).collect();
        push_history(&mut history, "newest");
        assert_eq!(history.len(), MAX_HISTORY);
        assert_eq!(history.last().unwrap(), "newest");
        assert_eq!(history.first().unwrap(), "cmd1");
    }

    #[test]
    fn save_history_atomic_writes_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("filecommand-history-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(HISTORY_FILE);
        let entries = vec!["dir".to_string(), "cd ..".to_string()];
        save_history_atomic(&path, &entries).expect("write history");
        assert_eq!(load_history(&path), entries);
        assert!(!path.with_file_name("history.json.tmp").exists(), "temp file must be renamed away");

        // Overwriting an existing file must succeed (rename-over semantics).
        let entries2 = vec!["echo hi".to_string()];
        save_history_atomic(&path, &entries2).expect("overwrite history");
        assert_eq!(load_history(&path), entries2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_history_missing_file_is_empty() {
        assert!(load_history(Path::new("no/such/history.json")).is_empty());
    }

    // -------------------------------------------------------------------
    // history.json (combined form): commands + directory frecency
    // (task 15.4)
    // -------------------------------------------------------------------

    fn sample_history_file() -> HistoryFile {
        HistoryFile {
            commands: vec!["dir".to_string(), r#"echo "hi""#.to_string()],
            directories: vec![
                FrecencyEntry { path: PathBuf::from(r"C:\Users\Me\Documents"), visit_count: 3, last_visited_ms: 12_345 },
                FrecencyEntry { path: PathBuf::from(r"C:\Users\Me\Downloads"), visit_count: 1, last_visited_ms: 999 },
            ],
        }
    }

    #[test]
    fn history_file_round_trips_commands_and_directories() {
        let file = sample_history_file();
        let rendered = render_history_file(&file);
        assert_eq!(parse_history_file(&rendered), file);
    }

    #[test]
    fn empty_history_file_round_trips() {
        let file = HistoryFile::default();
        let rendered = render_history_file(&file);
        assert_eq!(parse_history_file(&rendered), file);
    }

    #[test]
    fn history_file_paths_with_backslashes_round_trip() {
        // Windows paths are the norm here; the escaped backslashes must
        // survive the JSON round trip intact.
        let file = HistoryFile {
            commands: vec![],
            directories: vec![FrecencyEntry { path: PathBuf::from(r"C:\A\B\C"), visit_count: 5, last_visited_ms: 42 }],
        };
        let rendered = render_history_file(&file);
        assert_eq!(parse_history_file(&rendered), file);
    }

    #[test]
    fn malformed_history_file_content_yields_empty_history() {
        assert_eq!(parse_history_file("not json at all"), HistoryFile::default());
        assert_eq!(parse_history_file(""), HistoryFile::default());
    }

    #[test]
    fn partially_malformed_directories_key_falls_back_only_for_that_key() {
        // The commands array parses fine even though "directories" isn't a
        // bracketed value at all -- a partial failure degrades gracefully
        // rather than discarding the whole document.
        let input = "{\n  \"commands\": [\"dir\"],\n  \"directories\": null\n}\n";
        let parsed = parse_history_file(input);
        assert_eq!(parsed.commands, vec!["dir".to_string()]);
        assert!(parsed.directories.is_empty());
    }

    #[test]
    fn save_and_load_history_file_atomic_round_trips_and_overwrites() {
        let dir = std::env::temp_dir().join(format!("filecommand-history-file-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(HISTORY_FILE);

        let file = sample_history_file();
        save_history_file_atomic(&path, &file).expect("write combined history");
        assert_eq!(load_history_file(&path), file);
        assert!(!path.with_file_name("history.json.tmp").exists());

        let file2 = HistoryFile { commands: vec!["only".to_string()], directories: vec![] };
        save_history_file_atomic(&path, &file2).expect("overwrite combined history");
        assert_eq!(load_history_file(&path), file2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // -------------------------------------------------------------------
    // history-persistence-hardening: format marker, migration, cap, merge,
    // durable write (fuzzy-jump "Frecency ranking", "Directory history
    // persistence"; design D2, D3, D5, D6)
    // -------------------------------------------------------------------

    fn dir(path: &str, count: u32, ms: u64) -> FrecencyEntry {
        FrecencyEntry { path: PathBuf::from(path), visit_count: count, last_visited_ms: ms }
    }

    const LEGACY_FILE: &str = "{\n  \"commands\": [\"dir\"],\n  \"directories\": [\n    {\"path\": \"C:\\\\a\", \"visit_count\": 3, \"last_visited_ms\": 12345},\n    {\"path\": \"C:\\\\b\", \"visit_count\": 1, \"last_visited_ms\": 999}\n  ]\n}\n";

    #[test]
    fn render_writes_the_version_marker_and_a_round_trip_is_not_legacy() {
        let rendered = render_history_file(&sample_history_file());
        assert!(rendered.starts_with("{\n  \"version\": 2,\n  \"commands\": "), "{rendered}");
        let loaded = parse_history_document(&rendered);
        assert!(!loaded.legacy);
        assert_eq!(loaded.file, sample_history_file());
        assert!(!parse_history_document(&render_history_file(&HistoryFile::default())).legacy);
    }

    #[test]
    fn a_file_without_the_version_key_is_legacy_and_a_version_inside_a_string_does_not_count() {
        assert!(parse_history_document(LEGACY_FILE).legacy);
        assert_eq!(parse_history_document(LEGACY_FILE).file.commands, vec!["dir".to_string()]);
        let tricky = "{\n  \"commands\": [\"echo \\\"version\\\": 2\"],\n  \"directories\": []\n}\n";
        assert!(parse_history_document(tricky).legacy, "the marker is only ever a top-level key");
        assert_eq!(parse_history_document(tricky).file.commands, vec!["echo \"version\": 2".to_string()]);
        assert!(parse_history_document("not json at all").legacy);
        assert_eq!(parse_history_document("not json at all").file, HistoryFile::default(), "the existing malformed-file tolerance is unchanged");
        assert_eq!(load_history_document(Path::new("no/such/history.json")), HistoryLoad::default(), "a missing file is empty and not legacy");
    }

    #[test]
    fn migrate_restamps_every_legacy_visit_to_now() {
        let mut loaded = parse_history_document(LEGACY_FILE);
        let now = 1_787_000_000_000;
        migrate_loaded_history(&mut loaded.file, loaded.legacy, now);
        assert_eq!(loaded.file.directories, vec![dir(r"C:\a", 3, now), dir(r"C:\b", 1, now)], "counts carry over, times become the load time");
    }

    #[test]
    fn migrate_keeps_v2_timestamps_except_future_ones_which_clamp_to_now() {
        let now = 1_787_000_000_000;
        let mut file = HistoryFile {
            commands: vec![],
            directories: vec![dir(r"C:\past", 2, now - 5_000), dir(r"C:\future", 2, now + 60_000), dir(r"C:\now", 1, now)],
        };
        migrate_loaded_history(&mut file, false, now);
        assert_eq!(file.directories, vec![dir(r"C:\past", 2, now - 5_000), dir(r"C:\future", 2, now), dir(r"C:\now", 1, now)]);
    }

    #[test]
    fn migrate_prunes_an_oversized_file_to_the_cap() {
        let now = 1_787_000_000_000;
        let directories: Vec<FrecencyEntry> = (0..1_250).map(|i| dir(&format!(r"C:\d{i}"), i as u32 + 1, now)).collect();
        let mut file = HistoryFile { commands: vec![], directories };
        migrate_loaded_history(&mut file, false, now);
        assert_eq!(file.directories.len(), quicksearch::MAX_DIR_HISTORY);
        assert!(file.directories.iter().all(|d| d.visit_count > 250), "the 250 lowest-frecency entries were the ones dropped");
    }

    #[test]
    fn merge_unions_disjoint_directories_with_this_instances_entries_first() {
        let now = 1_000;
        let disk = HistoryFile { commands: vec![], directories: vec![dir(r"C:\disk-only", 2, 10)] };
        let mine = HistoryFile { commands: vec![], directories: vec![dir(r"C:\mine-1", 1, 20), dir(r"C:\mine-2", 1, 30)] };
        let merged = merge_history_files(&disk, &mine, now);
        assert_eq!(merged.directories, vec![dir(r"C:\mine-1", 1, 20), dir(r"C:\mine-2", 1, 30), dir(r"C:\disk-only", 2, 10)]);
    }

    /// fuzzy-jump "Merge keeps the larger count and the later visit".
    #[test]
    fn merge_takes_max_count_and_max_time_for_an_overlapping_path_and_is_idempotent() {
        let now = 5_000;
        let disk = HistoryFile { commands: vec![], directories: vec![dir(r"C:\src", 9, 1_000)] };
        let mine = HistoryFile { commands: vec![], directories: vec![dir(r"C:\src", 4, 2_000)] };
        let merged = merge_history_files(&disk, &mine, now);
        assert_eq!(merged.directories, vec![dir(r"C:\src", 9, 2_000)]);
        let again = merge_history_files(&merged, &mine, now);
        assert_eq!(again, merged, "merging the result with `mine` again changes nothing");
        let and_again = merge_history_files(&again, &mine, now);
        assert_eq!(and_again, merged);
    }

    /// fuzzy-jump "Another instance's commands are kept".
    #[test]
    fn merge_puts_disk_only_commands_first_without_duplicates_and_keeps_the_newest_two_hundred() {
        let disk = HistoryFile { commands: vec!["cd D:\\build".to_string(), "shared".to_string(), "older".to_string()], directories: vec![] };
        let mine = HistoryFile { commands: vec!["shared".to_string(), "mine-newest".to_string()], directories: vec![] };
        let merged = merge_history_files(&disk, &mine, 0);
        assert_eq!(merged.commands, vec!["cd D:\\build".to_string(), "older".to_string(), "shared".to_string(), "mine-newest".to_string()]);

        let disk = HistoryFile { commands: (0..150).map(|i| format!("disk{i}")).collect(), directories: vec![] };
        let mine = HistoryFile { commands: (0..100).map(|i| format!("mine{i}")).collect(), directories: vec![] };
        let merged = merge_history_files(&disk, &mine, 0);
        assert_eq!(merged.commands.len(), MAX_HISTORY);
        assert_eq!(merged.commands.last().unwrap(), "mine99", "the newest command is kept");
        assert_eq!(merged.commands[0], "disk50", "the oldest disk-only commands are dropped first");
    }

    #[test]
    fn merge_prunes_a_directory_union_above_the_cap() {
        let now = 1_787_000_000_000;
        let disk = HistoryFile { commands: vec![], directories: (0..700).map(|i| dir(&format!(r"C:\disk{i}"), 5, now)).collect() };
        let mine = HistoryFile { commands: vec![], directories: (0..700).map(|i| dir(&format!(r"C:\mine{i}"), 1, now)).collect() };
        let merged = merge_history_files(&disk, &mine, now);
        assert_eq!(merged.directories.len(), quicksearch::MAX_DIR_HISTORY);
        assert_eq!(merged.directories.iter().filter(|d| d.visit_count == 5).count(), 700, "the higher-frecency disk entries all survive");
    }

    /// fuzzy-jump "Interrupted write leaves a complete file": a failure at
    /// the replace step (the target held open with sharing denied) leaves
    /// the previous `history.json` byte-identical and no `.fc-tmp` sibling.
    #[cfg(windows)]
    #[test]
    fn save_history_file_atomic_failure_leaves_the_previous_file_and_no_temp() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp_dir("history-write-fails");
        let path = dir.join(HISTORY_FILE);
        save_history_file_atomic(&path, &sample_history_file()).unwrap();
        let original = std::fs::read(&path).unwrap();
        let lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).expect("open history.json with sharing denied");
        save_history_file_atomic(&path, &HistoryFile::default()).expect_err("the replace must fail while the target is locked");
        drop(lock);
        assert_eq!(std::fs::read(&path).unwrap(), original, "the previous file is byte-identical");
        assert_eq!(leftover_temp_files(&dir), Vec::<String>::new(), "no .fc-tmp sibling remains");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_writes_leave_no_shared_temp_name_behind() {
        let dir = temp_dir("history-write-temps");
        let path = dir.join(HISTORY_FILE);
        save_history_file_atomic(&path, &sample_history_file()).unwrap();
        save_history_atomic(&dir.join("plain.json"), &["dir".to_string()]).unwrap();
        let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(leftover_temp_files(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_history_file_missing_or_malformed_is_empty() {
        assert_eq!(load_history_file(Path::new("no/such/history.json")), HistoryFile::default());

        let dir = std::env::temp_dir().join(format!("filecommand-history-file-test-malformed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(HISTORY_FILE);
        std::fs::write(&path, "definitely not json").unwrap();
        assert_eq!(load_history_file(&path), HistoryFile::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -------------------------------------------------------------------
    // usermenu.toml
    // -------------------------------------------------------------------

    #[test]
    fn well_formed_entries_are_loaded_in_file_order() {
        let input = "[[entry]]\nlabel = \"A\"\ncommand = \"cmd-a\"\n\n[[entry]]\nlabel = \"B\"\ncommand = \"cmd-b\"\n";
        let parsed = parse_usermenu(input);
        assert!(!parsed.malformed);
        assert!(parsed.warnings.is_empty());
        assert_eq!(
            parsed.entries,
            vec![
                UserMenuEntry { label: "A".to_string(), command: "cmd-a".to_string() },
                UserMenuEntry { label: "B".to_string(), command: "cmd-b".to_string() },
            ]
        );
    }

    #[test]
    fn unknown_key_on_an_entry_warns_but_still_loads_it() {
        let input = "[[entry]]\nlabel = \"Backup\"\ncommand = \"robocopy . D:\\\\backup /E\"\nicon = \"disk\"\n";
        let parsed = parse_usermenu(input);
        assert!(!parsed.malformed, "an unknown key is a warning, not a hard failure");
        assert_eq!(parsed.entries, vec![UserMenuEntry { label: "Backup".to_string(), command: "robocopy . D:\\backup /E".to_string() }]);
        assert!(parsed.warnings.iter().any(|w| w.message.contains("icon")), "expected a warning mentioning the unknown key: {:?}", parsed.warnings);
        assert_eq!(parsed.warnings[0].to_string(), "usermenu.toml line 4: unknown key \"icon\" in an entry; ignored");
    }

    // -------------------------------------------------------------------
    // usermenu.toml on the shared tokenizer (configuration-files
    // "BOM-prefixed usermenu.toml loads its entries", "Inline comment in
    // usermenu.toml"; design D3)
    // -------------------------------------------------------------------

    #[test]
    fn usermenu_accepts_a_bom_trailing_comments_and_a_commented_header() {
        let input = "\u{feff}[[entry]] # first\nlabel = \"Build\" # main build\ncommand = 'cargo build' # c\n";
        let parsed = parse_usermenu(input);
        assert!(!parsed.malformed, "{parsed:?}");
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.entries, vec![UserMenuEntry { label: "Build".to_string(), command: "cargo build".to_string() }]);
    }

    #[test]
    fn usermenu_escaped_quote_does_not_close_the_string() {
        let parsed = parse_usermenu("[[entry]]\nlabel = \"Say \\\"hi\\\"\" # c\ncommand = \"echo \\\"hi\\\" # not a comment\"\n");
        assert!(!parsed.malformed, "{parsed:?}");
        assert_eq!(parsed.entries, vec![UserMenuEntry { label: "Say \"hi\"".to_string(), command: "echo \"hi\" # not a comment".to_string() }]);
    }

    #[test]
    fn usermenu_other_tables_and_unterminated_quotes_are_still_malformed() {
        assert!(parse_usermenu("[foo]\n").malformed, "[foo] is not [[entry]]");
        assert!(parse_usermenu("[[entry]]\nlabel = \"A\ncommand = \"a\"\n").malformed, "unterminated quote");
        assert!(parse_usermenu("[[entry]]\nlabel = A\ncommand = \"a\"\n").malformed, "bare value");
    }

    #[test]
    fn usermenu_warnings_carry_line_numbers() {
        let parsed = parse_usermenu("stray = \"x\"\n\n[[entry]]\nlabel = \"A\"\n\n[[entry]]\nlabel = \"B\"\ncommand = \"b\"\nicon = \"disk\"\n");
        assert!(!parsed.malformed);
        let rendered: Vec<String> = parsed.warnings.iter().map(ToString::to_string).collect();
        assert_eq!(
            rendered,
            vec![
                "usermenu.toml line 1: \"stray\" outside of an [[entry]] block; ignored".to_string(),
                "usermenu.toml line 3: entry missing \"command\"; skipped".to_string(),
                "usermenu.toml line 9: unknown key \"icon\" in an entry; ignored".to_string(),
            ]
        );
        assert_eq!(parsed.entries.len(), 1);
    }

    #[test]
    fn zero_entries_is_a_valid_empty_menu_not_malformed() {
        let parsed = parse_usermenu("");
        assert!(!parsed.malformed);
        assert!(parsed.entries.is_empty());
    }

    #[test]
    fn garbage_content_is_malformed() {
        let parsed = parse_usermenu("this is not valid toml at all\n[[entry]]\nlabel = \"A\"\ncommand = \"a\"\n");
        assert!(parsed.malformed);
    }

    #[test]
    fn an_unquoted_value_is_malformed() {
        // usermenu.toml is real TOML, unlike config.toml's lenient bare
        // values — an unquoted value is a parse error here.
        let parsed = parse_usermenu("[[entry]]\nlabel = A\ncommand = \"a\"\n");
        assert!(parsed.malformed);
    }

    #[test]
    fn an_entry_missing_a_required_field_is_dropped_with_a_warning() {
        let parsed = parse_usermenu("[[entry]]\nlabel = \"No command\"\n\n[[entry]]\nlabel = \"Complete\"\ncommand = \"ok\"\n");
        assert!(!parsed.malformed);
        assert_eq!(parsed.entries, vec![UserMenuEntry { label: "Complete".to_string(), command: "ok".to_string() }]);
        assert!(!parsed.warnings.is_empty());
    }

    #[test]
    fn usermenu_render_and_parse_round_trip_including_backslashes_and_quotes() {
        let entries = vec![
            UserMenuEntry { label: "Backup".to_string(), command: r"robocopy . D:\backup /E".to_string() },
            UserMenuEntry { label: "Say \"hi\"".to_string(), command: "echo \"hi\"".to_string() },
        ];
        let rendered = render_usermenu(&entries);
        let parsed = parse_usermenu(&rendered);
        assert!(!parsed.malformed);
        assert_eq!(parsed.entries, entries);
    }

    #[test]
    fn load_user_menu_creates_default_file_when_missing() {
        let dir = std::env::temp_dir().join(format!("filecommand-usermenu-test-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(USERMENU_FILE);
        let result = load_user_menu(&path);
        assert!(result.created_default);
        assert!(!result.malformed);
        assert_eq!(result.entries, default_user_menu_entries());
        // The file now exists on disk and reloading it yields the same
        // entries without creating it again.
        let reloaded = load_user_menu(&path);
        assert!(!reloaded.created_default);
        assert_eq!(reloaded.entries, default_user_menu_entries());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_user_menu_falls_back_without_overwriting_a_malformed_file() {
        let dir = std::env::temp_dir().join(format!("filecommand-usermenu-test-malformed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(USERMENU_FILE);
        let garbage = "not valid toml at all";
        std::fs::write(&path, garbage).unwrap();

        let result = load_user_menu(&path);
        assert!(result.malformed);
        assert_eq!(result.problem, Some(UserMenuProblem::InvalidToml));
        assert!(!result.created_default);
        assert_eq!(result.entries, default_user_menu_entries());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), garbage, "a malformed file must be left untouched on disk");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// configuration-files "Non-UTF-8 usermenu.toml is kept, not
    /// overwritten": a UTF-16LE save and a Windows-1252 save are each
    /// malformed (not missing), fall back to the defaults, and are left
    /// byte-for-byte on disk.
    #[test]
    fn load_user_menu_never_overwrites_a_non_utf8_file() {
        let utf16: Vec<u8> =
            [0xFFu8, 0xFE].into_iter().chain("[[entry]]\nlabel = \"A\"\ncommand = \"a\"\n".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        let ansi = b"[[entry]]\nlabel = \"Caf\xe9\"\ncommand = \"a\"\n".to_vec();
        for (name, bytes) in [("utf16", utf16), ("ansi", ansi)] {
            let dir = temp_dir(&format!("usermenu-{name}"));
            let path = dir.join(USERMENU_FILE);
            std::fs::write(&path, &bytes).unwrap();
            let result = load_user_menu(&path);
            assert!(result.malformed, "{name}: treated as malformed");
            assert_eq!(result.problem, Some(UserMenuProblem::NotUtf8), "{name}");
            assert!(!result.created_default, "{name}: not treated as missing");
            assert_eq!(result.entries, default_user_menu_entries());
            assert!(result.warnings.is_empty());
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "{name}: bytes on disk unchanged");
            assert_eq!(leftover_temp_files(&dir), Vec::<String>::new());
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn load_user_menu_default_file_is_written_durably_with_no_temp_left() {
        let dir = temp_dir("usermenu-default-durable");
        let path = dir.join(USERMENU_FILE);
        let result = load_user_menu(&path);
        assert!(result.created_default);
        assert_eq!(result.problem, None);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), render_usermenu(&default_user_menu_entries()));
        assert_eq!(leftover_temp_files(&dir), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_user_menu_returns_well_formed_entries_unchanged() {
        let dir = std::env::temp_dir().join(format!("filecommand-usermenu-test-wellformed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(USERMENU_FILE);
        std::fs::write(&path, "[[entry]]\nlabel = \"Only\"\ncommand = \"only-cmd\"\n").unwrap();

        let result = load_user_menu(&path);
        assert!(!result.malformed);
        assert!(!result.created_default);
        assert_eq!(result.entries, vec![UserMenuEntry { label: "Only".to_string(), command: "only-cmd".to_string() }]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -------------------------------------------------------------------
    // Configuration profile directory (config-profile-directory "Legacy
    // files in the working directory produce a notice, not a migration";
    // design D3)
    // -------------------------------------------------------------------

    /// A `cwd`/`profile` pair of empty directories under one uniquely
    /// named temp root, returned with the root so the caller can clean up.
    fn legacy_fixture(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("filecommand-legacy-{name}-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cwd = root.join("cwd");
        let profile = root.join("profile");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&profile).unwrap();
        (root, cwd, profile)
    }

    #[test]
    fn legacy_files_present_reports_each_file_individually() {
        for (name, file) in [("config", "config.toml"), ("history", HISTORY_FILE), ("usermenu", USERMENU_FILE)] {
            let (root, cwd, profile) = legacy_fixture(name);
            std::fs::write(cwd.join(file), "").unwrap();
            assert_eq!(legacy_files_present(&cwd, &profile), vec![file], "{file} alone is reported");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn legacy_files_present_lists_all_three_in_order() {
        let (root, cwd, profile) = legacy_fixture("all");
        for file in ["config.toml", HISTORY_FILE, USERMENU_FILE] {
            std::fs::write(cwd.join(file), "").unwrap();
        }
        assert_eq!(legacy_files_present(&cwd, &profile), vec!["config.toml", HISTORY_FILE, USERMENU_FILE]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_files_present_is_empty_when_cwd_has_none_of_the_files() {
        let (root, cwd, profile) = legacy_fixture("none");
        std::fs::write(cwd.join("unrelated.txt"), "").unwrap();
        assert_eq!(legacy_files_present(&cwd, &profile), Vec::<&'static str>::new());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_files_present_is_empty_when_the_profile_already_has_a_config() {
        let (root, cwd, profile) = legacy_fixture("populated");
        std::fs::write(cwd.join("config.toml"), "").unwrap();
        std::fs::write(cwd.join(HISTORY_FILE), "").unwrap();
        std::fs::write(profile.join("config.toml"), "").unwrap();
        assert_eq!(legacy_files_present(&cwd, &profile), Vec::<&'static str>::new(), "a populated profile means the user already moved on");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_files_present_is_empty_when_cwd_is_the_profile() {
        // Portable mode and the last-resort cwd fallback: the files in the
        // profile directory itself are the live configuration, not legacy
        // copies — a `history.json` there must not trigger the notice.
        let (root, cwd, _profile) = legacy_fixture("samedir");
        std::fs::write(cwd.join(HISTORY_FILE), "").unwrap();
        assert_eq!(legacy_files_present(&cwd, &cwd), Vec::<&'static str>::new());
        let _ = std::fs::remove_dir_all(&root);
    }

    // -------------------------------------------------------------------
    // Shared identity strings (help-and-about "Identity header matches the
    // shared source of truth"; user-menu is unrelated but this lives in the
    // same `config`-adjacent test surface per task 15.7).
    // -------------------------------------------------------------------

    #[test]
    fn identity_lines_are_the_single_source_of_truth_across_callers() {
        // `identity::identity_lines` is what app.rs (splash) and
        // info_panel.rs (the Info-mode banner) both consume verbatim — this
        // assertion pins that the four lines are stable in shape and order,
        // so a caller-side literal copy (as info_panel's tests use for
        // convenience) cannot silently drift from the real source.
        let lines = crate::identity::identity_lines(2026);
        assert_eq!(lines[0], crate::identity::PRODUCT_NAME);
        assert_eq!(lines[1], crate::identity::version_line());
        assert_eq!(lines[2], crate::identity::copyright_line(2026));
        assert_eq!(lines[3], crate::identity::TRIBUTE_LINE);
    }
}
