//! The F3 read-only viewer's core logic: instant-open byte access, the
//! terminal-independent state machine, and the pure text/hex layout,
//! backward-scan, and streaming-search algorithms.
//!
//! Nothing in this module depends on `ratatui` or `crossterm`. Rendering
//! (`filecommand-tui`'s `views/viewer`) is a pure function of a
//! [`state::ViewerState`] plus a byte window read through
//! [`byte_source::ByteSource`] — the module split mirrors that: state and
//! layout are unit-testable against temp files without a terminal (design
//! D1, D8).

pub mod backward;
pub mod byte_source;
pub mod decode;
pub mod forward;
pub mod hex;
pub mod search;
pub mod state;

pub use byte_source::ByteSource;
pub use state::{percent_through, ViewMode, ViewerState};
