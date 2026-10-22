//! The optional diagnostic log (error-diagnostics; application-shell
//! "Optional diagnostic log"): a minimal append-only file appender behind a
//! process-wide `OnceLock`, enabled at launch by `--log <path>` or the
//! `FILECOMMAND_LOG` environment variable. When neither is set the global
//! stays unset and every `log` call is a no-op that creates nothing.
//!
//! Deliberately dependency-free: the audit (B-06) asked for a diagnostic
//! trail without pulling in `log`/`tracing` (design D1), and
//! `filecommand-core` stays I/O-free — only the TUI writes here. Only
//! failures are recorded, so the single `Mutex` is never meaningfully
//! contended; the mutex is what makes the log safe to call from worker
//! threads.

use std::fmt::Display;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use filecommand_core::listing::DateTime;

/// The process-wide log, set once by [`init`]. A disabled log costs one
/// failed `get()` load per call; append mode keeps concurrent sessions that
/// share one file from clobbering each other.
static LOG: OnceLock<Logger> = OnceLock::new();

/// An open diagnostic log: the file plus its write lock, factored out of
/// the global so unit tests can drive one with an injected path (task 1.2).
struct Logger {
    file: Mutex<File>,
}

impl Logger {
    /// Open `path` in append mode — creating it when missing, never
    /// truncating it ("Existing log content is kept") — and write the
    /// session header line.
    fn open(path: &Path) -> io::Result<Logger> {
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        write_line(&mut file, "session", format_args!("FileCommand {} session start", env!("CARGO_PKG_VERSION")))?;
        Ok(Logger { file: Mutex::new(file) })
    }

    fn log(&self, category: &str, message: impl Display) {
        // A poisoned lock still yields the file: one panicking writer must
        // not disable diagnostics for the rest of the session. Write errors
        // are ignored — "a failure to write a log line SHALL never
        // interrupt the session".
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        let _ = write_line(&mut file, category, message);
        let _ = file.flush();
    }
}

/// One log line: UTC timestamp, category, message (design D1).
fn write_line(file: &mut File, category: &str, message: impl Display) -> io::Result<()> {
    writeln!(file, "{} [{}] {}", timestamp(), category, message)
}

/// The UTC timestamp prefix, `YYYY-MM-DDTHH:MM:SS.mmmZ`: the civil date and
/// time from `listing::DateTime` (the conversion the panels already use —
/// it carries no seconds field), plus the seconds and millisecond remainder
/// from the raw duration (design D1).
fn timestamp() -> String {
    let now = SystemTime::now();
    let dt = DateTime::from_system_time(now);
    let dur = now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", dt.year, dt.month, dt.day, dt.hour, dt.minute, dur.as_secs() % 60, dur.subsec_millis())
}

/// Open the diagnostic log at `path`, enabling [`log`] for the rest of the
/// session. On error nothing is enabled and the caller surfaces the warning
/// (design D2: "the session continues without logging").
pub fn init(path: &Path) -> io::Result<()> {
    let logger = Logger::open(path)?;
    // The launch flag is resolved once at startup, so this is set at most
    // once in production; a stray second `init` keeps the first logger
    // rather than orphaning its file.
    let _ = LOG.set(logger);
    Ok(())
}

/// Append one line to the diagnostic log. A no-op when logging was never
/// enabled — no file is created or written ("No log unless asked for").
pub fn log(category: &str, message: impl Display) {
    if let Some(logger) = LOG.get() {
        logger.log(category, message);
    }
}

/// Log `result`'s error with `context` and unwrap it to an `Option`, so a
/// former `let _ = fallible()` site records the failure without gaining new
/// control flow (designs D1/D3).
pub fn log_err<T, E: Display>(category: &str, context: impl Display, result: Result<T, E>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(err) => {
            log(category, format_args!("{context}: {err}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp file per test; the parent directory is created and any
    /// leftover from a previous run is removed first.
    fn temp_log(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("filecommand-tui-diag-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("fc.log")
    }

    /// `YYYY-MM-DDTHH:MM:SS.mmmZ` — 24 characters of digits with fixed
    /// punctuation, followed by `" ["` (the category bracket).
    fn assert_timestamp_prefix(line: &str) {
        assert!(line.len() > 26, "a log line is longer than its timestamp: {line:?}");
        let (ts, rest) = line.split_at(24);
        assert!(rest.starts_with(" ["), "a space and the category follow the timestamp: {line:?}");
        for (i, b) in ts.bytes().enumerate() {
            match i {
                4 | 7 => assert_eq!(b, b'-', "date dashes in {ts:?}"),
                10 => assert_eq!(b, b'T', "date/time separator in {ts:?}"),
                13 | 16 => assert_eq!(b, b':', "time colons in {ts:?}"),
                19 => assert_eq!(b, b'.', "millisecond dot in {ts:?}"),
                23 => assert_eq!(b, b'Z', "UTC marker in {ts:?}"),
                _ => assert!(b.is_ascii_digit(), "digit at offset {i} in {ts:?}"),
            }
        }
    }

    #[test]
    fn a_disabled_logger_creates_no_file() {
        // "No log unless asked for": no test in this binary calls the global
        // `init`, so `log` must be a pure no-op that creates nothing.
        let path = temp_log("disabled");
        let dir = path.parent().unwrap().to_path_buf();
        let _ = std::fs::remove_file(&path);
        log("persist", "history write C:\\nowhere\\history.json: denied");
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "nothing may be created while the log is disabled");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_enabled_logger_appends_timestamped_lines() {
        let path = temp_log("lines");
        {
            let logger = Logger::open(&path).unwrap();
            logger.log("persist", "history write C:\\p\\history.json: Access is denied. (os error 5)");
            logger.log("channel", "listing chunk not delivered: event loop gone");
        }

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3, "the session header plus the two logged lines: {content:?}");
        for line in &lines {
            assert_timestamp_prefix(line);
        }
        assert!(lines[0].contains("[session]") && lines[0].contains("session start"), "the header opens the session: {}", lines[0]);
        assert!(
            lines[1].contains("[persist] history write C:\\p\\history.json: Access is denied. (os error 5)"),
            "category and message are kept verbatim: {}",
            lines[1]
        );
        assert!(lines[2].contains("[channel] listing chunk not delivered: event loop gone"), "{}", lines[2]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn reopening_an_existing_log_keeps_its_earlier_content() {
        let path = temp_log("append");
        {
            let logger = Logger::open(&path).unwrap();
            logger.log("startup", "first session warning");
        }
        let first_len = std::fs::read_to_string(&path).unwrap().len();

        // "Existing log content is kept": a later session appends after the
        // earlier content rather than truncating it.
        {
            let logger = Logger::open(&path).unwrap();
            logger.log("startup", "second session warning");
        }

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.len() > first_len, "the second session appends");
        assert!(content.contains("first session warning"), "the earlier lines survive: {content:?}");
        assert!(content.contains("second session warning"));
        assert_eq!(content.matches("session start").count(), 2, "each session writes its own header");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn concurrent_logging_from_four_threads_produces_whole_lines() {
        let path = temp_log("threads");
        let logger = std::sync::Arc::new(Logger::open(&path).unwrap());
        let mut handles = Vec::new();
        for thread in 0..4 {
            let logger = std::sync::Arc::clone(&logger);
            handles.push(std::thread::spawn(move || {
                for i in 0..50 {
                    logger.log("copy", format_args!("thread {thread} line {i}"));
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        drop(logger);

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1 + 4 * 50, "the header plus every line, none lost or torn");
        for line in &lines[1..] {
            assert_timestamp_prefix(line);
            assert!(line.contains("[copy] thread "), "a whole, untorn line from one thread: {line:?}");
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn init_on_an_unopenable_path_fails_without_enabling_the_log() {
        // "Unwritable log path warns and continues": the open error reaches
        // the caller, and nothing is enabled.
        let missing_dir = std::env::temp_dir().join(format!("filecommand-tui-diag-init-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&missing_dir);
        let err = init(&missing_dir.join("fc.log")).expect_err("a log inside a nonexistent directory cannot be opened");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn log_err_maps_ok_to_some_and_err_to_none() {
        // The mapping is all a disabled log can show; the file-writing half
        // is covered by the `Logger` tests above.
        let ok: io::Result<usize> = Ok(5);
        assert_eq!(log_err("persist", "history write /x", ok), Some(5));
        let err: io::Result<usize> = Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        assert_eq!(log_err("persist", "history write /x", err), None);
    }
}
