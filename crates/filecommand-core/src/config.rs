//! Minimal `config.toml` reader plus the on-disk persistence helpers.
//!
//! Recognized keys: `splash` (bool), `theme` (string), `shell` (string),
//! `editor` (string, the F4 external-editor command), and the overridable
//! bindings `key.paste_name` / `key.paste_path`. Missing files and
//! unrecognized or malformed lines are tolerated; unrecognized keys are
//! ignored.
//!
//! Command history persists to `history.json` next to the config, written
//! atomically (temp file + rename) so a crash mid-write can never leave a
//! truncated history behind.

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

/// Config-overridable bindings. Only the M3 paste bindings are overridable
/// so far; the rest of the key map is still compiled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    /// Pastes the cursor entry's file name. Default Ctrl+Enter — reliable on
    /// Windows (native console records), best-effort elsewhere.
    pub paste_name: KeyBinding,
    /// Pastes the cursor entry's full path. Default Ctrl+] (ASCII 0x1D),
    /// which every terminal delivers.
    pub paste_path: KeyBinding,
}

impl Default for Keys {
    fn default() -> Self {
        Keys { paste_name: KeyBinding::new(true, false, false, "enter"), paste_path: KeyBinding::new(true, false, false, "]") }
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
    fn default_bindings_are_ctrl_enter_and_ctrl_bracket() {
        let keys = Keys::default();
        assert_eq!(keys.paste_name, KeyBinding::new(true, false, false, "enter"));
        assert_eq!(keys.paste_path, KeyBinding::new(true, false, false, "]"));
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
}
