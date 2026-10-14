//! File operations engine: Copy/Move/Mkdir/Delete (F5/F6/F7/F8), built on a
//! narrow filesystem trait seam so the engine itself never touches
//! `std::fs` directly and can be driven deterministically in tests.

pub mod conflict;
pub mod dialog;
pub mod error;
pub mod fs;
pub mod job;
pub mod path;
pub mod worker;

pub use conflict::{ConflictChoice, ConflictInfo, ConflictPolicy, ConflictResolution};
pub use dialog::{DropButtons, FileOpSetup, RunningDialog};
pub use error::{ErrorChoice, ErrorInfo, ErrorPolicy, ErrorResolution, ItemKind, SkippedItem};
#[cfg(test)]
pub use fs::FakeFs;
pub use fs::{ChunkControl, CopyOptions, CopyReport, FileId, FileSystem, FsMetadata, RawEntry, RealFs, ReparseKind, Unrestored, UnrestoredKind};
pub use job::{CancelFlag, Job, JobKind, JobOutcome, JobSink, ProgressInfo, ProgressThrottle, SourceItem};
pub use worker::run_job;
