fn main() {
    if std::env::args().skip(1).any(|a| a == "-h" || a == "-?" || a == "--help") {
        print_usage();
        std::process::exit(0);
    }
    let launch = filecommand_tui::app::parse_launch_args(std::env::args().skip(1));
    if let Err(err) = filecommand_tui::app::run(launch) {
        eprintln!("filecommand: fatal error: {err}");
        std::process::exit(1);
    }
}

fn print_usage() {
    // Named here so a user can find their files without reading docs
    // (config-profile-directory "Usage names the profile directory";
    // design D4). Nothing is created — resolution only.
    let default_profile = filecommand_tui::app::platform_default_profile_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "the current directory (no platform config directory could be resolved)".to_string());
    println!(
        "FileCommand {version}
A keyboard-driven, dual-panel file manager for the terminal.

USAGE:
    filecommand [OPTIONS] [PATH]

OPTIONS:
    -h, -?, --help      Print this help and exit
    --theme <name>      Override the active theme (built-in or custom)
    --nomouse           Disable mouse capture
    --nosplash          Skip the startup splash screen
    --noanimations      Disable decorative UI animations
    --config-dir <path>  Use <path> for config.toml, history.json, usermenu.toml
    --log <path>        Append diagnostic messages to <path> (or set FILECOMMAND_LOG)

Press F1 inside the application for the full keyboard reference.

Default profile directory: {default_profile}",
        version = env!("CARGO_PKG_VERSION")
    );
}
