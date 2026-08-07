//! Minimal `config.toml` reader. Only two keys are recognized in M1:
//! `splash` (bool) and `theme` (string). Missing files and unrecognized or
//! malformed lines are tolerated; unrecognized keys are ignored.

use std::path::Path;

use crate::theme::DEFAULT_THEME_NAME;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub splash: bool,
    pub theme: String,
}

impl Default for Config {
    fn default() -> Self {
        Config { splash: true, theme: DEFAULT_THEME_NAME.to_string() }
    }
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

fn parse_string(value: &str) -> Option<String> {
    let value = value.trim();
    let stripped = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')));
    stripped.map(|s| s.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_empty() {
        let config = parse("");
        assert_eq!(config, Config::default());
        assert_eq!(config.theme, "nc-classic");
        assert!(config.splash);
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
}
