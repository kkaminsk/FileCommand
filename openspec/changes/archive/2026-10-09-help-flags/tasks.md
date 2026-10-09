## 1. Argument Parsing

- [x] 1.1 In `crates/filecommand-tui/src/main.rs`, check `std::env::args()` for `-h`, `-?`, or `--help` at the top of `main` before any call to `run()`
- [x] 1.2 If a help flag is found, call a `print_usage()` function and `std::process::exit(0)`

## 2. Usage Text

- [x] 2.1 Write a `print_usage()` function that emits the usage block to stdout via `println!`
- [x] 2.2 Include the binary name, version from `env!("CARGO_PKG_VERSION")`, synopsis, and flag table (`-h`/`-?`/`--help`, `--theme`, `--nomouse`, `--nosplash`)
- [x] 2.3 Include a closing line directing the user to press F1 inside the app for the full key reference

## 3. Tests

- [x] 3.1 Add an integration test (or `assert_cmd`-style binary test) that invokes the binary with `-h`, `-?`, and `--help` and asserts exit code 0 and that stdout contains the version string
- [x] 3.2 Verify the TUI is not launched when a help flag is passed (terminal not acquired — confirmed by the process exiting immediately)
