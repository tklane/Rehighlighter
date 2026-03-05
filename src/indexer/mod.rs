pub mod line_index;
pub mod mmap;

pub use line_index::FileIndex;
pub use mmap::MmapFile;

use crossbeam_channel::{Receiver, Sender};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

pub enum IndexerMessage {
    /// Partial results during indexing
    Progress {
        chunk: Vec<u64>,
    },
    /// Indexing complete
    Complete,
    /// Indexing failed
    Error(String),
}

pub struct IndexerHandle {
    pub receiver: Receiver<IndexerMessage>,
    pub stop_flag: Arc<AtomicBool>,
}

pub fn spawn_indexer(mmap: Arc<MmapFile>) -> IndexerHandle {
    let (tx, rx) = crossbeam_channel::bounded(64);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop = stop_flag.clone();

    std::thread::spawn(move || {
        index_file_background(&mmap, tx, stop);
    });

    IndexerHandle {
        receiver: rx,
        stop_flag,
    }
}

fn index_file_background(mmap: &MmapFile, tx: Sender<IndexerMessage>, stop: Arc<AtomicBool>) {
    const REPORT_INTERVAL: usize = 100_000;

    // We need to access the raw bytes. We do this by reading through the mmap
    // in chunks using memchr for SIMD newline scanning.
    let file_len = mmap.len() as usize;
    if file_len == 0 {
        let _ = tx.send(IndexerMessage::Complete);
        return;
    }

    // Read the entire file as bytes via the mmap
    let data: &[u8] = mmap.as_bytes();

    let mut chunk = Vec::with_capacity(REPORT_INTERVAL);
    let mut pos = 0usize;

    while pos < data.len() {
        if stop.load(Ordering::Relaxed) {
            return;
        }

        match memchr::memchr(b'\n', &data[pos..]) {
            Some(rel) => {
                let next_line_start = pos + rel + 1;
                chunk.push(next_line_start as u64);

                if chunk.len() >= REPORT_INTERVAL {
                    if tx.send(IndexerMessage::Progress {
                        chunk: std::mem::take(&mut chunk),
                    }).is_err() {
                        return;
                    }
                }
                pos = next_line_start;
            }
            None => break,
        }
    }

    if !chunk.is_empty() {
        let _ = tx.send(IndexerMessage::Progress { chunk });
    }

    let _ = tx.send(IndexerMessage::Complete);
}
