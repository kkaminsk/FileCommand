//! FileCommand terminal UI: event loop, rendering, and terminal ownership.
//! Depends on `filecommand-core` for all application logic; this crate only
//! adds the ratatui/crossterm-backed shell around it.

pub mod app;
pub mod clipboard;
pub mod clock;
pub mod hitmap;
pub mod input;
pub mod layout;
pub mod style;
pub mod terminal;
pub mod views;
pub mod worker;
