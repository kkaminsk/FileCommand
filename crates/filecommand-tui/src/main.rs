fn main() {
    let no_splash = std::env::args().skip(1).any(|a| a == "--nosplash");
    if let Err(err) = filecommand_tui::app::run(no_splash) {
        eprintln!("filecommand: fatal error: {err}");
        std::process::exit(1);
    }
}
