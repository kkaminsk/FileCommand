//! Shell invocation construction.
//!
//! This module builds *what* to run (`shell + args + user text`) and *where*
//! (the active panel's directory). It never spawns anything and has no
//! terminal dependency — `filecommand-tui` owns the terminal and performs the
//! suspended spawn, exactly as it owns the worker threads for `fs_ops` jobs.

use std::path::{Path, PathBuf};

use crate::config::{DEFAULT_UNIX_SHELL, DEFAULT_WINDOWS_SHELL};

/// The extensions Windows treats as directly executable when no `PATHEXT`
/// environment variable is readable, plus `.lnk` shortcuts which `cmd.exe`
/// resolves for us.
pub const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC";

/// The ambient shell settings `update` needs, snapshotted once at startup.
///
/// Reading `PATHEXT` (or the config file) inside `update` would make it a
/// function of the environment rather than of `State`; capturing both here
/// keeps the transition pure and lets tests pin them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellConfig {
    /// The `shell =` value verbatim, or `None` for the platform default.
    pub shell: Option<String>,
    /// The raw `;`-separated executable-extension list.
    pub pathext: String,
}

impl ShellConfig {
    /// Capture the configured shell plus the ambient `PATHEXT`.
    pub fn from_env(shell: Option<String>) -> ShellConfig {
        ShellConfig { shell, pathext: std::env::var("PATHEXT").unwrap_or_else(|_| DEFAULT_PATHEXT.to_string()) }
    }
}

impl Default for ShellConfig {
    fn default() -> Self {
        ShellConfig { shell: None, pathext: DEFAULT_PATHEXT.to_string() }
    }
}

/// A shell program plus the arguments that precede the user's command text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSpec {
    pub program: String,
    pub args: Vec<String>,
}

/// A fully-formed spawn request: program, complete argument list (shell args
/// followed by the user's command text as a single argument), and working
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// The platform default shell spec string, as it would appear after
/// `shell =` in `config.toml`.
pub fn platform_default_spec() -> &'static str {
    if cfg!(windows) {
        DEFAULT_WINDOWS_SHELL
    } else {
        DEFAULT_UNIX_SHELL
    }
}

/// Split a shell spec into program + args, honoring double quotes so a
/// program path containing spaces can be written `"C:\Program Files\x.exe" /C`.
pub fn split_spec(spec: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    for c in spec.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    parts.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        parts.push(current);
    }
    parts
}

/// The conventional "run this command string" flag(s) for a shell program,
/// used when `shell =` names a program without giving its own arguments.
fn conventional_args(program: &str) -> Vec<String> {
    let stem = Path::new(program)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| program.to_lowercase());
    match stem.as_str() {
        "cmd" => vec!["/C".to_string()],
        // `-NoLogo` keeps the banner out of the suspended terminal; the
        // 200 ms+ startup cost is the documented tradeoff of opting in.
        "powershell" | "pwsh" => vec!["-NoLogo".to_string(), "-Command".to_string()],
        _ => vec!["-c".to_string()],
    }
}

/// Resolve the configured `shell =` value (or the platform default when it
/// is absent) into a program + preceding arguments.
pub fn resolve_shell(configured: Option<&str>) -> ShellSpec {
    let spec = match configured.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => platform_default_spec(),
    };
    let mut parts = split_spec(spec);
    if parts.is_empty() {
        parts = split_spec(platform_default_spec());
    }
    let program = parts.remove(0);
    let args = if parts.is_empty() { conventional_args(&program) } else { parts };
    ShellSpec { program, args }
}

/// Build the full invocation for `user_text`, run from `cwd` (the active
/// panel's directory).
pub fn build_command(configured: Option<&str>, user_text: &str, cwd: &Path) -> Invocation {
    let spec = resolve_shell(configured);
    let mut args = spec.args;
    args.push(user_text.to_string());
    Invocation { program: spec.program, args, cwd: cwd.to_path_buf() }
}

/// Whether `name` names something the shell would execute directly: a
/// `PATHEXT` match or a `.lnk` shortcut. `pathext` is the raw `;`-separated
/// list, so callers can test against a fixed list rather than the ambient
/// environment.
pub fn is_executable_name(name: &str, pathext: &str) -> bool {
    let lower = name.to_lowercase();
    if lower.ends_with(".lnk") {
        return true;
    }
    pathext
        .split(';')
        .map(str::trim)
        .filter(|ext| !ext.is_empty())
        .any(|ext| {
            let ext = ext.to_lowercase();
            let ext = if ext.starts_with('.') { ext } else { format!(".{ext}") };
            lower.ends_with(&ext)
        })
}

/// Whether the entry under the cursor is an executable target, using the
/// ambient `PATHEXT` when readable and [`DEFAULT_PATHEXT`] otherwise.
pub fn is_executable_target(name: &str) -> bool {
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| DEFAULT_PATHEXT.to_string());
    is_executable_name(name, &pathext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_windows_shell_is_cmd_slash_c() {
        let spec = resolve_shell(None);
        if cfg!(windows) {
            assert_eq!(spec.program, "cmd.exe");
            assert_eq!(spec.args, vec!["/C".to_string()]);
        } else {
            assert_eq!(spec.program, "/bin/sh");
            assert_eq!(spec.args, vec!["-c".to_string()]);
        }
    }

    #[test]
    fn build_command_appends_user_text_and_uses_panel_cwd() {
        let inv = build_command(Some("cmd.exe /C"), "dir", Path::new(r"C:\NORTON"));
        assert_eq!(inv.program, "cmd.exe");
        assert_eq!(inv.args, vec!["/C".to_string(), "dir".to_string()]);
        assert_eq!(inv.cwd, PathBuf::from(r"C:\NORTON"));
    }

    #[test]
    fn configured_powershell_supplies_its_own_conventional_args() {
        let inv = build_command(Some("powershell"), "Get-ChildItem", Path::new(r"D:\WORK"));
        assert_eq!(inv.program, "powershell");
        assert_eq!(inv.args, vec!["-NoLogo".to_string(), "-Command".to_string(), "Get-ChildItem".to_string()]);
        assert_eq!(inv.cwd, PathBuf::from(r"D:\WORK"));
    }

    #[test]
    fn configured_pwsh_with_explicit_args_is_used_verbatim() {
        let spec = resolve_shell(Some("pwsh -NoProfile -Command"));
        assert_eq!(spec.program, "pwsh");
        assert_eq!(spec.args, vec!["-NoProfile".to_string(), "-Command".to_string()]);
    }

    #[test]
    fn quoted_program_path_with_spaces_stays_one_token() {
        let spec = resolve_shell(Some(r#""C:\Program Files\PowerShell\7\pwsh.exe" -Command"#));
        assert_eq!(spec.program, r"C:\Program Files\PowerShell\7\pwsh.exe");
        assert_eq!(spec.args, vec!["-Command".to_string()]);
    }

    #[test]
    fn blank_or_whitespace_shell_config_falls_back_to_platform_default() {
        assert_eq!(resolve_shell(Some("   ")), resolve_shell(None));
        assert_eq!(resolve_shell(Some("")), resolve_shell(None));
    }

    #[test]
    fn command_construction_needs_no_terminal_or_spawn() {
        // The whole point of D1: this is a pure value, computable in a unit
        // test with no console, no child process, and no filesystem access.
        let inv = build_command(None, "dir", Path::new("/somewhere"));
        assert_eq!(inv.args.last().unwrap(), "dir");
        assert_eq!(inv.cwd, PathBuf::from("/somewhere"));
    }

    #[test]
    fn executable_targets_match_pathext_and_lnk() {
        assert!(is_executable_name("setup.exe", DEFAULT_PATHEXT));
        assert!(is_executable_name("BUILD.BAT", DEFAULT_PATHEXT));
        assert!(is_executable_name("Shortcut.lnk", DEFAULT_PATHEXT));
        assert!(!is_executable_name("readme.txt", DEFAULT_PATHEXT));
        assert!(!is_executable_name("noextension", DEFAULT_PATHEXT));
    }

    #[test]
    fn pathext_entries_without_leading_dot_still_match() {
        assert!(is_executable_name("tool.ps1", "PS1;EXE"));
    }

    #[test]
    fn split_spec_drops_empty_runs() {
        assert_eq!(split_spec("  cmd.exe   /C  "), vec!["cmd.exe".to_string(), "/C".to_string()]);
        assert!(split_spec("   ").is_empty());
    }
}
