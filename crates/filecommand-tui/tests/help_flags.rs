/// Integration tests for -h / -? / --help flags (help-flags OpenSpec change).
///
/// Uses CARGO_BIN_EXE_filecommand (set by cargo at test build time) to invoke
/// the real binary and assert exit-code 0 + version string in stdout without
/// the TUI ever being launched.

const VERSION: &str = env!("CARGO_PKG_VERSION");
const BIN: &str = env!("CARGO_BIN_EXE_filecommand");

fn run(arg: &str) -> std::process::Output {
    std::process::Command::new(BIN)
        .arg(arg)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn {BIN}: {e}"))
}

#[test]
fn help_short_h_exits_zero() {
    let out = run("-h");
    assert!(out.status.success(), "-h should exit 0, got {:?}", out.status.code());
}

#[test]
fn help_short_h_prints_version() {
    let out = run("-h");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(VERSION), "-h stdout should contain version {VERSION}:\n{stdout}");
}

#[test]
fn help_question_mark_exits_zero() {
    let out = run("-?");
    assert!(out.status.success(), "-? should exit 0, got {:?}", out.status.code());
}

#[test]
fn help_question_mark_prints_version() {
    let out = run("-?");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(VERSION), "-? stdout should contain version {VERSION}:\n{stdout}");
}

#[test]
fn help_long_exits_zero() {
    let out = run("--help");
    assert!(out.status.success(), "--help should exit 0, got {:?}", out.status.code());
}

#[test]
fn help_long_prints_version() {
    let out = run("--help");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(VERSION), "--help stdout should contain version {VERSION}:\n{stdout}");
}
