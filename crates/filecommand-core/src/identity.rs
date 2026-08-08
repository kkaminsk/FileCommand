//! Product identity strings — single source of truth for splash/about text.

/// The product name as displayed to the user.
pub const PRODUCT_NAME: &str = "FileCommand";

/// The tribute line honoring the project's inspiration.
pub const TRIBUTE_LINE: &str = "Inspired by the Norton Commander, 1986-1998";

/// The crate version, taken from Cargo.toml at compile time.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// "Version X.Y.Z"
pub fn version_line() -> String {
    format!("Version {}", version())
}

/// "Copyright (C) <year> The FileCommand Authors"
pub fn copyright_line(year: i32) -> String {
    format!("Copyright (C) {year} The FileCommand Authors")
}

/// The four identity lines shown on the splash screen, in display order:
/// product name, version, copyright, tribute.
pub fn identity_lines(year: i32) -> [String; 4] {
    [
        PRODUCT_NAME.to_string(),
        version_line(),
        copyright_line(year),
        TRIBUTE_LINE.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_includes_crate_version() {
        assert_eq!(version_line(), format!("Version {}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn copyright_line_includes_year_and_authors() {
        assert_eq!(copyright_line(2026), "Copyright (C) 2026 The FileCommand Authors");
    }

    #[test]
    fn identity_lines_order() {
        let lines = identity_lines(2026);
        assert_eq!(lines[0], PRODUCT_NAME);
        assert!(lines[1].starts_with("Version "));
        assert!(lines[2].starts_with("Copyright (C) 2026"));
        assert_eq!(lines[3], TRIBUTE_LINE);
    }
}
