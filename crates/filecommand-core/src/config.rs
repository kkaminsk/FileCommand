//! Minimal `config.toml` reader plus the on-disk persistence helpers.
//!
//! Recognized keys: `splash` (bool), `theme` (string), `shell` (string),
//! `editor` (string, the F4 external-editor command), and the overridable
//! bindings `key.paste_name` / `key.paste_path` / `key.quick_filter` /
//! `key.fuzzy_jump`. Missing files and unrecognized or malformed lines are
//! tolerated; unrecognized keys are ignored.
//!
//! Command history persists to `history.json` next to the config, written
//! atomically (temp file + rename) so a crash mid-write can never leave a
//! truncated history behind.
//!
//! The F2 user menu is loaded from `usermenu.toml`, a real (if minimal)
//! TOML array-of-tables document — unlike `config.toml`/`history.json`
//! above, which are deliberately *not* real TOML/JSON parsers. Unlike those
//! two, malformed `usermenu.toml` content is treated as an error condition
//! (falling back to defaults and warning) rather than silently tolerated,
//! per user-menu "Create and recover the usermenu.toml file".

use std::io;
use std::path::Path;

use crate::theme::DEFAULT_THEME_NAME;

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
}

impl Default for Keys {
    fn default() -> Self {
        Keys {
            paste_name: KeyBinding::new(true, false, false, "enter"),
            paste_path: KeyBinding::new(true, false, false, "]"),
            quick_filter: KeyBinding::new(true, false, false, "p"),
            fuzzy_jump: KeyBinding::new(true, false, false, "j"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub splash: bool,
    pub theme: String,
    /// `shell = ` verbatim. `None` means "use the platform default"; see
    /// [`crate::shell::resolve_shell`].
    pub shell: Option<String>,
    /// `editor = ` verbatim, the F4 external-editor command. `None` (also
    /// yielded by a blank/empty value) means unset: F4 shows a "no editor
    /// configured" message instead of spawning anything (external-editor:
    /// Config-driven external editor command — "Editor command unset").
    pub editor: Option<String>,
    pub keys: Keys,
}

impl Default for Config {
    fn default() -> Self {
        Config { splash: true, theme: DEFAULT_THEME_NAME.to_string(), shell: None, editor: None, keys: Keys::default() }
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

/// Parse a minimal `key = value` TOML-ish document. Tolerant: blank lines,
/// `#` comments, and unrecognized keys/malformed lines are silently
/// skipped rather than causing a parse failure.
pub fn parse(input: &str) -> Config {
    let mut config = Config::default();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        let value = value.trim();
        match key {
            "splash" => {
                if let Some(b) = parse_bool(value) {
                    config.splash = b;
                }
            }
            "theme" => {
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
            "key.paste_name" => {
                if let Some(b) = parse_string(value).as_deref().and_then(parse_binding) {
                    config.keys.paste_name = b;
                }
            }
            "key.paste_path" => {
                if let Some(b) = parse_string(value).as_deref().and_then(parse_binding) {
                    config.keys.paste_path = b;
                }
            }
            "key.quick_filter" => {
                if let Some(b) = parse_string(value).as_deref().and_then(parse_binding) {
                    config.keys.quick_filter = b;
                }
            }
            "key.fuzzy_jump" => {
                if let Some(b) = parse_string(value).as_deref().and_then(parse_binding) {
                    config.keys.fuzzy_jump = b;
                }
            }
            _ => {}
        }
    }
    config
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Accept both quoted (`"cmd.exe /C"`) and bare (`cmd.exe /C`) values — the
/// shell spec is far more readable unquoted, and this parser has never been
/// strict TOML anyway.
fn parse_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let stripped = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')));
    Some(stripped.unwrap_or(value).to_string())
}

/// Load config from `path`. Tolerant of a missing file — returns
/// [`Config::default`] in that case, matching "missing config is not an
/// error" behavior expected at startup.
pub fn load(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(contents) => parse(&contents),
        Err(_) => Config::default(),
    }
}

// ---------------------------------------------------------------------
// history.json
// ---------------------------------------------------------------------

/// Render history entries as a JSON array of strings. Hand-rolled rather
/// than pulled from `serde` — this is the only JSON in the workspace and the
/// shape is one string array.
pub fn render_history(entries: &[String]) -> String {
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
    out.push_str("]\n");
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
pub fn parse_history(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut s = String::new();
        loop {
            match chars.next() {
                None => return out,
                Some('"') => break,
                Some('\\') => match chars.next() {
                    Some('n') => s.push('\n'),
                    Some('r') => s.push('\r'),
                    Some('t') => s.push('\t'),
                    Some('u') => {
                        let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                        match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            Some(c) => s.push(c),
                            None => return out,
                        }
                    }
                    Some(other) => s.push(other),
                    None => return out,
                },
                Some(other) => s.push(other),
            }
        }
        out.push(s);
    }
    out
}

/// Read persisted history. A missing or unreadable file is an empty
/// history, never an error.
pub fn load_history(path: &Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => parse_history(&contents),
        Err(_) => Vec::new(),
    }
}

/// Write history atomically: serialize to a sibling temp file, then rename
/// over the target. `std::fs::rename` replaces an existing file on both
/// Windows and Unix, so a reader never observes a partial write.
pub fn save_history_atomic(path: &Path, entries: &[String]) -> io::Result<()> {
    let tmp = temp_sibling(path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&tmp, render_history(entries))?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn temp_sibling(path: &Path) -> std::path::PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| std::ffi::OsString::from("history.json"));
    name.push(".tmp");
    match path.parent() {
        Some(parent) => parent.join(name),
        None => std::path::PathBuf::from(name),
    }
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
/// entry, or a key found outside any `[[entry]]` block). `malformed` is set
/// when the content contains a line that is neither blank, a comment, an
/// `[[entry]]` header, nor a `key = "value"` pair — real invalid TOML,
/// distinct from a merely-unrecognized key (user-menu "Unknown keys warn
/// rather than fail" vs "Malformed file warns and falls back").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUserMenu {
    pub entries: Vec<UserMenuEntry>,
    pub warnings: Vec<String>,
    pub malformed: bool,
}

/// Parse `usermenu.toml` content: a sequence of `[[entry]]` blocks, each
/// with `label = "..."` and `command = "..."` string keys (basic
/// double-quoted or literal single-quoted TOML strings). Pure and
/// terminal-free, so parsing is unit-testable without touching a file
/// (user-menu "Parsing SHALL be performed by the `config` module ... MUST
/// be unit-testable without a terminal").
pub fn parse_usermenu(input: &str) -> ParsedUserMenu {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut malformed = false;
    let mut current: Option<(Option<String>, Option<String>)> = None;
    let mut in_entry = false;

    for raw_line in input.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[entry]]" {
            flush_usermenu_entry(&mut current, &mut entries, &mut warnings);
            current = Some((None, None));
            in_entry = true;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            malformed = true;
            continue;
        };
        let key = key.trim();
        let Some(value) = parse_toml_string_strict(value.trim()) else {
            malformed = true;
            continue;
        };
        if !in_entry {
            warnings.push(format!("`{key}` outside of an [[entry]] block is ignored"));
            continue;
        }
        let Some((label, command)) = current.as_mut() else { continue };
        match key {
            "label" => *label = Some(value),
            "command" => *command = Some(value),
            other => warnings.push(format!("unknown key `{other}` in a usermenu.toml entry")),
        }
    }
    flush_usermenu_entry(&mut current, &mut entries, &mut warnings);

    ParsedUserMenu { entries, warnings, malformed }
}

/// Close out the entry block being accumulated (if any), pushing it as a
/// completed [`UserMenuEntry`] when both `label` and `command` were set, or
/// warning and dropping it when either is missing.
fn flush_usermenu_entry(current: &mut Option<(Option<String>, Option<String>)>, entries: &mut Vec<UserMenuEntry>, warnings: &mut Vec<String>) {
    let Some((label, command)) = current.take() else { return };
    match (label, command) {
        (Some(label), Some(command)) => entries.push(UserMenuEntry { label, command }),
        (label, _) => {
            let missing = if label.is_none() { "label" } else { "command" };
            warnings.push(format!("entry missing `{missing}`, skipped"));
        }
    }
}

/// Parse a strict basic (`"..."`, backslash escapes) or literal (`'...'`,
/// no escapes) TOML string. Unlike [`parse_string`], a bare/unquoted value
/// is rejected (`None`) rather than accepted — `usermenu.toml` is real
/// TOML, so an unquoted value is malformed content, not a readability
/// shorthand.
fn parse_toml_string_strict(value: &str) -> Option<String> {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        unescape_toml_basic(&value[1..value.len() - 1])
    } else if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
        Some(value[1..value.len() - 1].to_string())
    } else {
        None
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

/// The outcome of loading `usermenu.toml` at startup (user-menu "Create and
/// recover the usermenu.toml file").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMenuLoadResult {
    pub entries: Vec<UserMenuEntry>,
    /// Non-fatal parse warnings (empty unless `Loaded`-equivalent content
    /// had unknown keys).
    pub warnings: Vec<String>,
    /// The file did not exist (or could not be read) and default entries
    /// were written to `path`.
    pub created_default: bool,
    /// The file existed but its content was malformed; defaults are in
    /// effect in memory and the on-disk file was left untouched — a
    /// startup warning should be shown to the user.
    pub malformed: bool,
}

/// Load the F2 user menu from `path`. Three outcomes (user-menu "Missing
/// file is created with defaults", "Malformed file warns and falls back
/// without overwriting", well-formed load):
///
/// - Missing/unreadable file: default entries are written to `path` and
///   returned (`created_default: true`).
/// - Malformed content: default entries are returned without touching the
///   file (`malformed: true`).
/// - Well-formed content: the parsed entries (possibly with warnings) are
///   returned, including the zero-entries case (a deliberately empty menu
///   is not malformed).
pub fn load_user_menu(path: &Path) -> UserMenuLoadResult {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => {
            let entries = default_user_menu_entries();
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            let _ = std::fs::write(path, render_usermenu(&entries));
            return UserMenuLoadResult { entries, warnings: Vec::new(), created_default: true, malformed: false };
        }
    };
    let parsed = parse_usermenu(&contents);
    if parsed.malformed {
        return UserMenuLoadResult { entries: default_user_menu_entries(), warnings: Vec::new(), created_default: false, malformed: true };
    }
    UserMenuLoadResult { entries: parsed.entries, warnings: parsed.warnings, created_default: false, malformed: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_empty() {
        let config = parse("");
        assert_eq!(config, Config::default());
        assert_eq!(config.theme, "nc-classic");
        assert!(config.splash);
        assert_eq!(config.shell, None);
    }

    #[test]
    fn parses_splash_and_theme() {
        let config = parse("splash = false\ntheme = \"nc-mono\"\n");
        assert!(!config.splash);
        assert_eq!(config.theme, "nc-mono");
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
        let config = load(Path::new("this/path/definitely/does/not/exist/config.toml"));
        assert_eq!(config, Config::default());
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
    }

    #[test]
    fn parse_binding_handles_modifiers_and_bare_keys() {
        assert_eq!(parse_binding("ctrl+enter"), Some(KeyBinding::new(true, false, false, "enter")));
        assert_eq!(parse_binding("F9"), Some(KeyBinding::new(false, false, false, "f9")));
        assert_eq!(parse_binding("ctrl++"), Some(KeyBinding::new(true, false, false, "+")));
        assert_eq!(parse_binding("ctrl"), None);
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
        assert!(parsed.warnings.iter().any(|w| w.contains("icon")), "expected a warning mentioning the unknown key: {:?}", parsed.warnings);
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
        assert!(!result.created_default);
        assert_eq!(result.entries, default_user_menu_entries());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), garbage, "a malformed file must be left untouched on disk");
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
