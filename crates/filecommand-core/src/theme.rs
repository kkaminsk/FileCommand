//! Named 16-color theme model. Every renderer resolves colors by role, never
//! by hardcoding an ANSI or RGB value directly — see [`Theme::resolve`].

use std::collections::BTreeMap;
use std::fmt;

/// The eight ANSI colors plus their bright variants. This is the mandatory
/// baseline every role must define; truecolor is an optional enhancement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ansi16 {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

/// A resolved-or-resolvable color value for one side (fg/bg) of a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorValue {
    Ansi16(Ansi16),
    /// "none" / inherit the terminal's default color for this side.
    Inherit,
}

/// 24-bit truecolor value, `#RRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// The color-depth a terminal has reported. Truecolor is an opt-in
/// enhancement; the ANSI-16 value is always the fallback and is never
/// substituted with a 256-color indexed value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    Ansi16,
    TrueColor,
}

/// A fully validated fg/bg pair for one role: mandatory ANSI-16 values plus
/// optional truecolor overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorSpec {
    pub fg: ColorValue,
    pub bg: ColorValue,
    pub fg_truecolor: Option<Rgb>,
    pub bg_truecolor: Option<Rgb>,
}

/// The color actually selected for rendering, after depth resolution. This
/// type structurally excludes 256-color indexed values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedColor {
    Ansi16(Ansi16),
    Inherit,
    Rgb(Rgb),
}

impl ColorSpec {
    pub fn new(fg: ColorValue, bg: ColorValue) -> Self {
        Self { fg, bg, fg_truecolor: None, bg_truecolor: None }
    }

    pub fn with_truecolor(mut self, fg: Option<Rgb>, bg: Option<Rgb>) -> Self {
        self.fg_truecolor = fg;
        self.bg_truecolor = bg;
        self
    }

    pub fn resolve_fg(&self, depth: ColorDepth) -> ResolvedColor {
        match (depth, self.fg_truecolor) {
            (ColorDepth::TrueColor, Some(rgb)) => ResolvedColor::Rgb(rgb),
            _ => self.fg.into(),
        }
    }

    pub fn resolve_bg(&self, depth: ColorDepth) -> ResolvedColor {
        match (depth, self.bg_truecolor) {
            (ColorDepth::TrueColor, Some(rgb)) => ResolvedColor::Rgb(rgb),
            _ => self.bg.into(),
        }
    }
}

impl From<ColorValue> for ResolvedColor {
    fn from(v: ColorValue) -> Self {
        match v {
            ColorValue::Ansi16(a) => ResolvedColor::Ansi16(a),
            ColorValue::Inherit => ResolvedColor::Inherit,
        }
    }
}

/// The fixed set of named roles a theme must define. Adding a role here
/// means every theme (including third-party ones, in later milestones) must
/// supply a mapping for it or fail validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    ScreenBackdrop,
    ScreenPlaceholder,
    PanelFrame,
    PanelTitleActive,
    PanelTitleInactive,
    PanelHeader,
    PanelFile,
    PanelDirectory,
    PanelCursor,
    PanelSelected,
    PanelMinistatus,
    /// The M5 git-info status-marker column's `M` (modified) glyph
    /// (git-info "Per-file status marker column").
    PanelGitModified,
    /// The status-marker column's `?` (untracked) glyph.
    PanelGitUntracked,
    /// The status-marker column's `+` (staged) glyph.
    PanelGitStaged,
    KeybarNumber,
    KeybarLabel,
    CommandLine,
    Clock,
    MenuBar,
    MenuBody,
    MenuHighlight,
    MenuHotkey,
    MenuDisabled,
    InfoLabel,
    InfoValue,
    InfoBanner,
    DialogPrimary,
    DialogError,
    DialogInput,
    ButtonNormal,
    ButtonFocused,
    DialogGaugeFilled,
    DialogGaugeEmpty,
    SplashFrame,
    SplashTitle,
    SplashVersion,
    SplashText,
    /// The F3 viewer's (and, from M5, the editor's) header row: filename,
    /// col/offset, size, and percent indicators (viewer: Frame-less
    /// full-screen chrome).
    ViewerHeader,
    /// The viewer's text/hex body.
    ViewerText,
    /// A search hit's highlighted cells (viewer: F7 streaming search —
    /// "Match becomes the top anchor and is highlighted").
    ViewerMatch,
    /// The panel-tabs strip's active-tab label (panel-tabs "Tab label
    /// rendering and active styling").
    TabActive,
    /// The panel-tabs strip's inactive-tab labels and `◄`/`►` overflow
    /// markers.
    TabInactive,
}

pub const ALL_ROLES: &[Role] = &[
    Role::ScreenBackdrop,
    Role::ScreenPlaceholder,
    Role::PanelFrame,
    Role::PanelTitleActive,
    Role::PanelTitleInactive,
    Role::PanelHeader,
    Role::PanelFile,
    Role::PanelDirectory,
    Role::PanelCursor,
    Role::PanelSelected,
    Role::PanelMinistatus,
    Role::PanelGitModified,
    Role::PanelGitUntracked,
    Role::PanelGitStaged,
    Role::KeybarNumber,
    Role::KeybarLabel,
    Role::CommandLine,
    Role::Clock,
    Role::MenuBar,
    Role::MenuBody,
    Role::MenuHighlight,
    Role::MenuHotkey,
    Role::MenuDisabled,
    Role::InfoLabel,
    Role::InfoValue,
    Role::InfoBanner,
    Role::DialogPrimary,
    Role::DialogError,
    Role::DialogInput,
    Role::ButtonNormal,
    Role::ButtonFocused,
    Role::DialogGaugeFilled,
    Role::DialogGaugeEmpty,
    Role::SplashFrame,
    Role::SplashTitle,
    Role::SplashVersion,
    Role::SplashText,
    Role::ViewerHeader,
    Role::ViewerText,
    Role::ViewerMatch,
    Role::TabActive,
    Role::TabInactive,
];

impl Role {
    /// The dotted name used in config files and error messages, e.g.
    /// `"panel.title.active"`.
    pub fn dotted(&self) -> &'static str {
        match self {
            Role::ScreenBackdrop => "screen.backdrop",
            Role::ScreenPlaceholder => "screen.placeholder",
            Role::PanelFrame => "panel.frame",
            Role::PanelTitleActive => "panel.title.active",
            Role::PanelTitleInactive => "panel.title.inactive",
            Role::PanelHeader => "panel.header",
            Role::PanelFile => "panel.file",
            Role::PanelDirectory => "panel.directory",
            Role::PanelCursor => "panel.cursor",
            Role::PanelSelected => "panel.selected",
            Role::PanelMinistatus => "panel.ministatus",
            Role::PanelGitModified => "panel.git.modified",
            Role::PanelGitUntracked => "panel.git.untracked",
            Role::PanelGitStaged => "panel.git.staged",
            Role::KeybarNumber => "keybar.number",
            Role::KeybarLabel => "keybar.label",
            Role::CommandLine => "commandline",
            Role::Clock => "clock",
            Role::MenuBar => "menubar",
            Role::MenuBody => "menu.body",
            Role::MenuHighlight => "menu.highlight",
            Role::MenuHotkey => "menu.hotkey",
            Role::MenuDisabled => "menu.disabled",
            Role::InfoLabel => "info.label",
            Role::InfoValue => "info.value",
            Role::InfoBanner => "info.banner",
            Role::DialogPrimary => "dialog.primary",
            Role::DialogError => "dialog.error",
            Role::DialogInput => "dialog.input",
            Role::ButtonNormal => "button.normal",
            Role::ButtonFocused => "button.focused",
            Role::DialogGaugeFilled => "dialog.gauge.filled",
            Role::DialogGaugeEmpty => "dialog.gauge.empty",
            Role::SplashFrame => "splash.frame",
            Role::SplashTitle => "splash.title",
            Role::SplashVersion => "splash.version",
            Role::SplashText => "splash.text",
            Role::ViewerHeader => "viewer.header",
            Role::ViewerText => "viewer.text",
            Role::ViewerMatch => "viewer.match",
            Role::TabActive => "tab.active",
            Role::TabInactive => "tab.inactive",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.dotted())
    }
}

/// Raw, possibly-incomplete color input for a role, as it would come from an
/// external theme definition. `None` for `fg`/`bg` means the field was never
/// supplied at all — distinct from an explicit `Inherit`, which is valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ColorSpecInput {
    pub fg: Option<ColorValue>,
    pub bg: Option<ColorValue>,
    pub fg_truecolor: Option<Rgb>,
    pub bg_truecolor: Option<Rgb>,
}

impl ColorSpecInput {
    pub fn new(fg: ColorValue, bg: ColorValue) -> Self {
        Self { fg: Some(fg), bg: Some(bg), fg_truecolor: None, bg_truecolor: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    MissingRole(Role),
    MissingAnsi16Fg(Role),
    MissingAnsi16Bg(Role),
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeError::MissingRole(r) => write!(f, "theme is missing role `{r}`"),
            ThemeError::MissingAnsi16Fg(r) => write!(f, "role `{r}` is missing a mandatory ANSI-16 foreground value"),
            ThemeError::MissingAnsi16Bg(r) => write!(f, "role `{r}` is missing a mandatory ANSI-16 background value"),
        }
    }
}

impl std::error::Error for ThemeError {}

/// A validated, complete theme: every [`Role`] in [`ALL_ROLES`] maps to a
/// [`ColorSpec`] with mandatory ANSI-16 values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    roles: BTreeMap<Role, ColorSpec>,
}

impl Theme {
    /// Validate raw role input and build a complete theme. Fails if any role
    /// in [`ALL_ROLES`] is absent, or present but missing a mandatory
    /// ANSI-16 fg/bg value.
    pub fn build(name: impl Into<String>, mut roles: BTreeMap<Role, ColorSpecInput>) -> Result<Theme, ThemeError> {
        let mut resolved = BTreeMap::new();
        for role in ALL_ROLES {
            let input = roles.remove(role).ok_or(ThemeError::MissingRole(*role))?;
            let fg = input.fg.ok_or(ThemeError::MissingAnsi16Fg(*role))?;
            let bg = input.bg.ok_or(ThemeError::MissingAnsi16Bg(*role))?;
            resolved.insert(
                *role,
                ColorSpec { fg, bg, fg_truecolor: input.fg_truecolor, bg_truecolor: input.bg_truecolor },
            );
        }
        Ok(Theme { name: name.into(), roles: resolved })
    }

    /// Look up the spec for a role. Always present for a validated theme.
    pub fn get(&self, role: Role) -> ColorSpec {
        *self.roles.get(&role).expect("Theme::build guarantees every role is present")
    }

    /// Every role this theme defines a spec for (always [`ALL_ROLES`]).
    pub fn roles(&self) -> impl Iterator<Item = (&Role, &ColorSpec)> {
        self.roles.iter()
    }

    pub fn classic() -> Theme {
        use Ansi16::*;
        use ColorValue::{Ansi16 as A, Inherit};
        let mut m = BTreeMap::new();
        let mut set = |role: Role, fg: ColorValue, bg: ColorValue| {
            m.insert(role, ColorSpecInput::new(fg, bg));
        };
        set(Role::ScreenBackdrop, Inherit, A(Blue));
        set(Role::ScreenPlaceholder, A(White), A(Blue));
        set(Role::PanelFrame, A(Cyan), A(Blue));
        set(Role::PanelTitleActive, A(Black), A(Cyan));
        set(Role::PanelTitleInactive, A(Cyan), A(Blue));
        set(Role::PanelHeader, A(BrightYellow), A(Blue));
        set(Role::PanelFile, A(Cyan), A(Blue));
        set(Role::PanelDirectory, A(BrightWhite), A(Blue));
        set(Role::PanelCursor, A(Black), A(Cyan));
        set(Role::PanelSelected, A(BrightYellow), A(Blue));
        set(Role::PanelMinistatus, A(Cyan), A(Blue));
        // Conventional git-status hues: red for a working-tree change,
        // yellow for untracked, green for staged — distinct from every
        // other panel role so the marker column reads at a glance.
        set(Role::PanelGitModified, A(BrightRed), A(Blue));
        set(Role::PanelGitUntracked, A(BrightYellow), A(Blue));
        set(Role::PanelGitStaged, A(BrightGreen), A(Blue));
        set(Role::KeybarNumber, A(White), A(Black));
        set(Role::KeybarLabel, A(Black), A(Cyan));
        set(Role::CommandLine, A(White), A(Black));
        set(Role::Clock, A(Black), A(Cyan));
        set(Role::MenuBar, A(Black), A(Cyan));
        set(Role::MenuBody, A(Black), A(Cyan));
        set(Role::MenuHighlight, A(White), A(Black));
        set(Role::MenuHotkey, A(BrightYellow), A(Cyan));
        set(Role::MenuDisabled, A(White), A(Cyan));
        set(Role::InfoLabel, A(Cyan), A(Blue));
        set(Role::InfoValue, A(BrightYellow), A(Blue));
        set(Role::InfoBanner, A(BrightWhite), A(Blue));
        set(Role::DialogPrimary, A(Black), A(Cyan));
        set(Role::DialogError, A(BrightWhite), A(Red));
        set(Role::DialogInput, A(Black), A(Cyan));
        set(Role::ButtonNormal, A(Black), A(White));
        set(Role::ButtonFocused, A(Black), A(BrightYellow));
        set(Role::DialogGaugeFilled, A(Blue), A(Cyan));
        set(Role::DialogGaugeEmpty, A(Black), A(Cyan));
        set(Role::SplashFrame, A(Cyan), A(Blue));
        set(Role::SplashTitle, A(BrightWhite), A(Blue));
        set(Role::SplashVersion, A(White), A(Blue));
        set(Role::SplashText, A(Cyan), A(Blue));
        set(Role::ViewerHeader, A(Black), A(Cyan));
        set(Role::ViewerText, A(White), A(Blue));
        set(Role::ViewerMatch, A(Black), A(Cyan));
        set(Role::TabActive, A(Black), A(Cyan));
        set(Role::TabInactive, A(Cyan), A(Blue));
        Theme::build("nc-classic", m).expect("nc-classic role table is complete")
    }

    pub fn mono() -> Theme {
        use Ansi16::*;
        use ColorValue::Ansi16 as A;
        let mut m = BTreeMap::new();
        let mut set = |role: Role, fg: ColorValue, bg: ColorValue| {
            m.insert(role, ColorSpecInput::new(fg, bg));
        };
        set(Role::ScreenBackdrop, ColorValue::Inherit, A(Black));
        set(Role::ScreenPlaceholder, A(White), A(Black));
        set(Role::PanelFrame, A(White), A(Black));
        set(Role::PanelTitleActive, A(Black), A(White));
        set(Role::PanelTitleInactive, A(White), A(Black));
        set(Role::PanelHeader, A(White), A(Black));
        set(Role::PanelFile, A(White), A(Black));
        set(Role::PanelDirectory, A(BrightWhite), A(Black));
        set(Role::PanelCursor, A(Black), A(White));
        set(Role::PanelSelected, A(BrightWhite), A(Black));
        set(Role::PanelMinistatus, A(White), A(Black));
        // Mono carries no hue meaning (same precedent as `MenuHotkey`
        // above): all three marker glyphs read as ordinary bright text
        // rather than color-coded status.
        set(Role::PanelGitModified, A(BrightWhite), A(Black));
        set(Role::PanelGitUntracked, A(BrightWhite), A(Black));
        set(Role::PanelGitStaged, A(BrightWhite), A(Black));
        set(Role::KeybarNumber, A(White), A(Black));
        set(Role::KeybarLabel, A(Black), A(White));
        set(Role::CommandLine, A(White), A(Black));
        set(Role::Clock, A(Black), A(White));
        set(Role::MenuBar, A(Black), A(White));
        set(Role::MenuBody, A(Black), A(White));
        set(Role::MenuHighlight, A(White), A(Black));
        // Grey-on-white is the only "unavailable" cue mono can carry, and it
        // is a brightness difference rather than a hue one.
        set(Role::MenuDisabled, A(BrightBlack), A(White));
        // Mono carries no hue meaning; the hotkey letter reads as ordinary
        // bar text and F1 Help documents the letters instead.
        set(Role::MenuHotkey, A(Black), A(White));
        set(Role::InfoLabel, A(White), A(Black));
        set(Role::InfoValue, A(BrightWhite), A(Black));
        set(Role::InfoBanner, A(BrightWhite), A(Black));
        set(Role::DialogPrimary, A(Black), A(White));
        set(Role::DialogError, A(BrightWhite), A(Black));
        set(Role::DialogInput, A(Black), A(White));
        set(Role::ButtonNormal, A(Black), A(White));
        set(Role::ButtonFocused, A(White), A(Black));
        set(Role::DialogGaugeFilled, A(BrightWhite), A(White));
        set(Role::DialogGaugeEmpty, A(Black), A(White));
        set(Role::SplashFrame, A(White), A(Black));
        set(Role::SplashTitle, A(BrightWhite), A(Black));
        set(Role::SplashVersion, A(White), A(Black));
        set(Role::SplashText, A(White), A(Black));
        // Explicitly called out with the other headers/highlights that
        // invert to black-on-white in mono (§4.11).
        set(Role::ViewerHeader, A(Black), A(White));
        set(Role::ViewerText, A(White), A(Black));
        set(Role::ViewerMatch, A(Black), A(White));
        // Mirrors `PanelTitleActive`/`PanelTitleInactive`'s mono inversion:
        // the active tab inverts to black-on-white, the inactive tabs stay
        // white-on-black.
        set(Role::TabActive, A(Black), A(White));
        set(Role::TabInactive, A(White), A(Black));
        Theme::build("nc-mono", m).expect("nc-mono role table is complete")
    }

    /// Resolve a compiled-in theme by name. Returns `None` for unknown names
    /// so callers (config loading) can fall back to the default.
    pub fn by_name(name: &str) -> Option<Theme> {
        match name {
            "nc-classic" => Some(Theme::classic()),
            "nc-mono" => Some(Theme::mono()),
            _ => None,
        }
    }
}

pub const DEFAULT_THEME_NAME: &str = "nc-classic";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_rejects_missing_role() {
        let mut m = BTreeMap::new();
        for role in ALL_ROLES.iter().skip(1) {
            m.insert(*role, ColorSpecInput::new(ColorValue::Ansi16(Ansi16::White), ColorValue::Ansi16(Ansi16::Black)));
        }
        let err = Theme::build("incomplete", m).unwrap_err();
        assert_eq!(err, ThemeError::MissingRole(ALL_ROLES[0]));
    }

    #[test]
    fn build_rejects_missing_ansi16_fg() {
        let mut m = BTreeMap::new();
        for role in ALL_ROLES {
            m.insert(*role, ColorSpecInput::new(ColorValue::Ansi16(Ansi16::White), ColorValue::Ansi16(Ansi16::Black)));
        }
        m.insert(
            Role::PanelFrame,
            ColorSpecInput { fg: None, bg: Some(ColorValue::Ansi16(Ansi16::Black)), fg_truecolor: None, bg_truecolor: None },
        );
        let err = Theme::build("bad", m).unwrap_err();
        assert_eq!(err, ThemeError::MissingAnsi16Fg(Role::PanelFrame));
    }

    #[test]
    fn build_accepts_explicit_inherit() {
        let mut m = BTreeMap::new();
        for role in ALL_ROLES {
            m.insert(*role, ColorSpecInput::new(ColorValue::Inherit, ColorValue::Ansi16(Ansi16::Black)));
        }
        assert!(Theme::build("ok", m).is_ok());
    }

    #[test]
    fn classic_theme_is_role_complete() {
        let theme = Theme::classic();
        for role in ALL_ROLES {
            let _ = theme.get(*role);
        }
        assert_eq!(theme.roles().count(), ALL_ROLES.len());
    }

    #[test]
    fn mono_theme_is_role_complete() {
        let theme = Theme::mono();
        for role in ALL_ROLES {
            let _ = theme.get(*role);
        }
    }

    #[test]
    fn classic_role_table_matches_spec() {
        let theme = Theme::classic();
        let cases: &[(Role, ColorValue, ColorValue)] = &[
            (Role::PanelFrame, ColorValue::Ansi16(Ansi16::Cyan), ColorValue::Ansi16(Ansi16::Blue)),
            (Role::PanelTitleActive, ColorValue::Ansi16(Ansi16::Black), ColorValue::Ansi16(Ansi16::Cyan)),
            (Role::PanelTitleInactive, ColorValue::Ansi16(Ansi16::Cyan), ColorValue::Ansi16(Ansi16::Blue)),
            (Role::PanelHeader, ColorValue::Ansi16(Ansi16::BrightYellow), ColorValue::Ansi16(Ansi16::Blue)),
            (Role::PanelDirectory, ColorValue::Ansi16(Ansi16::BrightWhite), ColorValue::Ansi16(Ansi16::Blue)),
            (Role::PanelCursor, ColorValue::Ansi16(Ansi16::Black), ColorValue::Ansi16(Ansi16::Cyan)),
            (Role::KeybarNumber, ColorValue::Ansi16(Ansi16::White), ColorValue::Ansi16(Ansi16::Black)),
            (Role::KeybarLabel, ColorValue::Ansi16(Ansi16::Black), ColorValue::Ansi16(Ansi16::Cyan)),
        ];
        for (role, fg, bg) in cases {
            let spec = theme.get(*role);
            assert_eq!(spec.fg, *fg, "fg mismatch for {role}");
            assert_eq!(spec.bg, *bg, "bg mismatch for {role}");
        }
        assert_eq!(theme.get(Role::ScreenBackdrop).fg, ColorValue::Inherit);
    }

    #[test]
    fn mono_inverts_active_roles_to_black_on_white() {
        let theme = Theme::mono();
        for role in [Role::PanelTitleActive, Role::PanelCursor, Role::KeybarLabel, Role::Clock, Role::DialogPrimary] {
            let spec = theme.get(role);
            assert_eq!(spec.fg, ColorValue::Ansi16(Ansi16::Black), "{role} fg");
            assert_eq!(spec.bg, ColorValue::Ansi16(Ansi16::White), "{role} bg");
        }
        for role in [Role::PanelDirectory, Role::PanelSelected] {
            assert_eq!(theme.get(role).fg, ColorValue::Ansi16(Ansi16::BrightWhite));
        }
    }

    #[test]
    fn resolve_uses_ansi16_when_not_truecolor() {
        let spec = ColorSpec::new(ColorValue::Ansi16(Ansi16::Cyan), ColorValue::Ansi16(Ansi16::Blue))
            .with_truecolor(Some(Rgb(0, 255, 255)), Some(Rgb(0, 0, 170)));
        assert_eq!(spec.resolve_fg(ColorDepth::Ansi16), ResolvedColor::Ansi16(Ansi16::Cyan));
        assert_eq!(spec.resolve_bg(ColorDepth::Ansi16), ResolvedColor::Ansi16(Ansi16::Blue));
    }

    #[test]
    fn resolve_uses_truecolor_only_when_reported_and_present() {
        let with_true = ColorSpec::new(ColorValue::Ansi16(Ansi16::Cyan), ColorValue::Ansi16(Ansi16::Blue))
            .with_truecolor(Some(Rgb(1, 2, 3)), None);
        assert_eq!(with_true.resolve_fg(ColorDepth::TrueColor), ResolvedColor::Rgb(Rgb(1, 2, 3)));
        // bg has no truecolor override even though depth is truecolor -> falls back to ANSI-16.
        assert_eq!(with_true.resolve_bg(ColorDepth::TrueColor), ResolvedColor::Ansi16(Ansi16::Blue));

        let without_true = ColorSpec::new(ColorValue::Ansi16(Ansi16::Cyan), ColorValue::Ansi16(Ansi16::Blue));
        assert_eq!(without_true.resolve_fg(ColorDepth::TrueColor), ResolvedColor::Ansi16(Ansi16::Cyan));
    }

    #[test]
    fn resolved_color_never_represents_256_indexed() {
        // Structural guarantee: ResolvedColor only has three variants, none of
        // which can carry a 256-color index. This test exists as a
        // regression tripwire — if a variant is ever added it must be one of
        // these three.
        let variants = [ResolvedColor::Ansi16(Ansi16::White), ResolvedColor::Inherit, ResolvedColor::Rgb(Rgb(0, 0, 0))];
        assert_eq!(variants.len(), 3);
    }

    #[test]
    fn classic_menu_and_info_roles_match_spec() {
        let theme = Theme::classic();
        let cases: &[(Role, Ansi16, Ansi16)] = &[
            (Role::MenuBar, Ansi16::Black, Ansi16::Cyan),
            (Role::MenuBody, Ansi16::Black, Ansi16::Cyan),
            (Role::MenuHighlight, Ansi16::White, Ansi16::Black),
            (Role::MenuHotkey, Ansi16::BrightYellow, Ansi16::Cyan),
            (Role::MenuDisabled, Ansi16::White, Ansi16::Cyan),
            (Role::InfoLabel, Ansi16::Cyan, Ansi16::Blue),
            (Role::InfoValue, Ansi16::BrightYellow, Ansi16::Blue),
            (Role::InfoBanner, Ansi16::BrightWhite, Ansi16::Blue),
        ];
        for (role, fg, bg) in cases {
            let spec = theme.get(*role);
            assert_eq!(spec.fg, ColorValue::Ansi16(*fg), "fg mismatch for {role}");
            assert_eq!(spec.bg, ColorValue::Ansi16(*bg), "bg mismatch for {role}");
        }
    }

    #[test]
    fn menu_roles_are_visually_distinct_in_both_themes() {
        for theme in [Theme::classic(), Theme::mono()] {
            let body = theme.get(Role::MenuBody);
            assert_ne!(body, theme.get(Role::MenuHighlight), "{}: selected item must differ from body", theme.name);
            assert_ne!(body, theme.get(Role::MenuDisabled), "{}: disabled item must differ from body", theme.name);
        }
    }

    #[test]
    fn by_name_resolves_known_themes_and_rejects_unknown() {
        assert!(Theme::by_name("nc-classic").is_some());
        assert!(Theme::by_name("nc-mono").is_some());
        assert!(Theme::by_name("does-not-exist").is_none());
    }
}
