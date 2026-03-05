use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

use crate::{
    indexer::{spawn_indexer, FileIndex, IndexerHandle, IndexerMessage, MmapFile},
    overview_cache::{spawn_overview_cache, OverviewCache, OverviewCacheHandle},
    search::{spawn_search, SearchState},
    timestamp::{auto_bucket_secs, rebin_from_pairs, spawn_histogram, HistogramData, HistogramHandle},
};

#[derive(Clone, Debug, PartialEq)]
pub enum TabStatus {
    Indexing { progress_pct: f32 },
    Ready,
    Error(String),
}

pub struct TabState {
    pub id: Uuid,
    pub path: PathBuf,
    pub title: String,

    pub mmap: Arc<MmapFile>,
    pub index: FileIndex,

    pub indexer: Option<IndexerHandle>,
    pub status: TabStatus,

    /// Row to scroll to (consumed once per frame by the log view)
    pub scroll_to_row: Option<usize>,
    pub last_scroll_row: usize,

    pub search: SearchState,

    pub detail_line: Option<usize>,
    pub detail_open: bool,

    pub histogram_data: Option<HistogramData>,
    pub histogram_handle: Option<HistogramHandle>,
    pub show_histogram: bool,
    /// None = auto (based on data range). Some(secs) = user-selected bucket size.
    pub histogram_granularity: Option<i64>,

    pub overview_cache: Option<OverviewCache>,
    pub overview_handle: Option<OverviewCacheHandle>,

    /// Text copied from the log (⌘C), pending addition as a search term.
    pub pending_search_addition: Option<String>,
}

impl TabState {
    pub fn new(path: PathBuf, mmap: Arc<MmapFile>) -> Self {
        let file_size = mmap.len();
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());

        let index = FileIndex::new(file_size);
        let indexer = spawn_indexer(mmap.clone());

        Self {
            id: Uuid::new_v4(),
            path,
            title,
            mmap,
            index,
            indexer: Some(indexer),
            status: TabStatus::Indexing { progress_pct: 0.0 },
            scroll_to_row: None,
            last_scroll_row: 0,
            search: SearchState::default(),
            detail_line: None,
            detail_open: false,
            histogram_data: None,
            histogram_handle: None,
            show_histogram: false,
            histogram_granularity: None,
            overview_cache: None,
            overview_handle: None,
            pending_search_addition: None,
        }
    }

    /// Poll the indexer channel and integrate new line offsets.
    /// Returns true if state changed (caller should repaint).
    pub fn poll_indexer(&mut self) -> bool {
        let Some(ref handle) = self.indexer else {
            return false;
        };

        let mut changed = false;

        // Drain up to 16 messages per frame to avoid blocking UI
        for _ in 0..16 {
            match handle.receiver.try_recv() {
                Ok(IndexerMessage::Progress { chunk }) => {
                    let pct = if self.mmap.len() > 0 {
                        let approx = self.index.last_offset();
                        (approx as f32 / self.mmap.len() as f32).clamp(0.0, 1.0) * 100.0
                    } else {
                        0.0
                    };
                    self.index.extend_with_chunk(chunk);
                    self.status = TabStatus::Indexing { progress_pct: pct };
                    changed = true;
                }
                Ok(IndexerMessage::Complete) => {
                    self.index.finalize();
                    self.status = TabStatus::Ready;
                    self.indexer = None;
                    changed = true;
                    // Re-run search if one was pending
                    if !self.search.query.text.is_empty() || !self.search.query.terms.is_empty() {
                        self.trigger_search();
                    }
                    // Spawn histogram computation
                    let index_snap = Arc::new(self.index.clone_snapshot());
                    self.histogram_handle = Some(spawn_histogram(self.mmap.clone(), index_snap));
                    // Spawn overview cache computation
                    self.overview_handle =
                        Some(spawn_overview_cache(Arc::new(self.index.clone_snapshot())));
                    break;
                }
                Ok(IndexerMessage::Error(e)) => {
                    self.status = TabStatus::Error(e);
                    self.indexer = None;
                    changed = true;
                    break;
                }
                Err(_) => break,
            }
        }

        changed
    }

    /// Poll background search and update search state.
    /// Returns true if matches were updated.
    pub fn poll_search(&mut self) -> bool {
        let total = self.index.line_count();
        let updated = self.search.poll_search(total);
        if updated && !self.search.compiled_excludes.is_empty() {
            self.compute_display_lines();
        }
        updated
    }

    /// (Re)compute `search.display_lines` — the final list of visible lines
    /// after applying filter mode AND exclusion terms. Call whenever either changes.
    pub fn compute_display_lines(&mut self) {
        if self.search.compiled_excludes.is_empty() {
            self.search.display_lines = None;
            return;
        }

        let line_count = self.index.line_count();
        let base: Vec<usize> = if self.search.filter_mode {
            self.search
                .visible_lines
                .clone()
                .unwrap_or_else(|| (0..line_count).collect())
        } else {
            (0..line_count).collect()
        };

        let mut display = Vec::with_capacity(base.len());
        for line in base {
            let keep = if let Some(range) = self.index.line_byte_range(line) {
                let bytes = self.mmap.line_bytes(range);
                if let Ok(text) = std::str::from_utf8(bytes) {
                    !self.search.compiled_excludes.iter().any(|re| re.is_match(text))
                } else {
                    true
                }
            } else {
                true
            };
            if keep {
                display.push(line);
            }
        }
        self.search.display_lines = Some(display);
    }

    /// Trigger a background search with the current query.
    pub fn trigger_search(&mut self) {
        if self.search.query.text.is_empty() && self.search.query.terms.is_empty() {
            self.search.matching_lines.clear();
            self.search.visible_lines = None;
            self.search.current_match_index = None;
            return;
        }

        self.search.compile();
        if self.search.compile_error.is_some() {
            return;
        }

        let index = Arc::new(self.index.clone_snapshot());
        let mmap = self.mmap.clone();
        let query = self.search.query.clone();
        self.search.search_handle = Some(spawn_search(mmap, index, query));
    }

    /// Poll background histogram computation. Returns true if data arrived.
    pub fn poll_histogram(&mut self) -> bool {
        let result = {
            let Some(ref handle) = self.histogram_handle else { return false };
            handle.receiver.try_recv().ok()
        };
        if let Some(data) = result {
            self.histogram_data = Some(data);
            self.histogram_handle = None;
            return true;
        }
        false
    }

    /// Poll background overview cache computation. Returns true if data arrived.
    pub fn poll_overview_cache(&mut self) -> bool {
        let result = {
            let Some(ref handle) = self.overview_handle else {
                return false;
            };
            handle.receiver.try_recv().ok()
        };
        if let Some(data) = result {
            self.overview_cache = Some(data);
            self.overview_handle = None;
            return true;
        }
        false
    }

    /// Rebin the histogram with the given bucket size (or auto-select if None).
    /// No-op if no histogram data is available yet.
    pub fn rebin_histogram(&mut self, bucket_secs: Option<i64>) {
        self.histogram_granularity = bucket_secs;
        if let Some(ref data) = self.histogram_data.clone() {
            if data.pairs.is_empty() {
                return;
            }
            let secs = bucket_secs.unwrap_or_else(|| auto_bucket_secs(&data.pairs));
            self.histogram_data = Some(rebin_from_pairs(&data.pairs, secs, data.line_count));
        }
    }

    pub fn is_indexing(&self) -> bool {
        matches!(self.status, TabStatus::Indexing { .. })
    }
}
