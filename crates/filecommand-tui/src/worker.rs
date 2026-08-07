//! Worker-thread mechanism: fulfills `Effect::StartListing` by spawning a
//! background thread that streams directory entries back over a channel as
//! `Command`s, re-entering the same `update` path the main loop uses for
//! key-derived commands.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use filecommand_core::listing::{list_dir_chunked, Entry, StdFsReader};
use filecommand_core::panel::parent_path;
use filecommand_core::{Command, PanelSide};

const CHUNK_SIZE: usize = 256;

pub fn spawn_listing(panel: PanelSide, path: PathBuf, tx: Sender<Command>) {
    std::thread::spawn(move || {
        let mut total = 0usize;

        if parent_path(&path).is_some() {
            total += 1;
            if tx.send(Command::ListingChunk { panel, entries: vec![Entry::parent_dir()] }).is_err() {
                return;
            }
        }

        let reader = StdFsReader;
        let result = list_dir_chunked(&reader, &path, CHUNK_SIZE, |chunk| {
            let _ = tx.send(Command::ListingChunk { panel, entries: chunk });
        });

        match result {
            Ok(fs_count) => {
                let _ = tx.send(Command::ListingComplete { panel, total: total + fs_count });
            }
            Err(e) => {
                let _ = tx.send(Command::ListingFailed { panel, message: e.to_string() });
            }
        }
    });
}
