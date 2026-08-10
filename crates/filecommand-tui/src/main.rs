fn main() {
    let launch = filecommand_tui::app::parse_launch_args(std::env::args().skip(1));
    if let Err(err) = filecommand_tui::app::run(launch) {
        eprintln!("filecommand: fatal error: {err}");
        std::process::exit(1);
    }
}
