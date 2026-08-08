//! Platform-agnostic application logic for FileCommand.
//!
//! This crate has **no** dependency on `ratatui` or `crossterm` and performs
//! no terminal I/O. It owns the pure state-transition model
//! ([`update::update`]) plus the panel, listing, theme, config, clock, and
//! identity data types that model drives.

pub mod clock;
pub mod config;
pub mod drives;
pub mod external_editor;
pub mod fs_ops;
pub mod identity;
pub mod info;
pub mod listing;
pub mod menu;
pub mod panel;
pub mod shell;
pub mod theme;
pub mod update;
pub mod viewer;

pub use update::{update, Command, Effect, PanelSide, State, UiPhase};
