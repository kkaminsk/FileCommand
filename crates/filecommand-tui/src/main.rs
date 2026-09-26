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

Press F1 inside the application for the full keyboard reference.",
        version = env!("CARGO_PKG_VERSION")
    );
}
