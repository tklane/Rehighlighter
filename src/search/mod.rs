pub mod filter;

use regex::{Regex, RegexBuilder};
use std::sync::Arc;
use crossbeam_channel::{Receiver, Sender};

use crate::indexer::{FileIndex, MmapFile};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchQuery {
    pub text: String,
    /// Committed search terms (displayed as chips in the search bar).
    pub terms: Vec<String>,
    /// Lines matching any of these are hidden from the view.
    pub exclude_terms: Vec<String>,
    pub is_regex: bool,
    pub case_sensitive: bool,
    /// Number of lines before/after each match to show in filter mode
    pub context_lines: usize,
}

#[derive(Default)]
pub struct SearchState {
    pub query: SearchQuery,
    /// Compiled regex for the current text field.
    pub compiled: Option<Regex>,
    /// Compiled regexes for each committed term.
    pub compiled_terms: Vec<Regex>,
    pub compile_error: Option<String>,

    /// Sorted list of line indices that match the search
    pub matching_lines: Vec<usize>,

    /// In filter mode: matching lines expanded with context
    pub visible_lines: Option<Vec<usize>>,

    /// Compiled regexes for exclusion terms.
    pub compiled_excludes: Vec<Regex>,

    /// Lines to display after applying exclusion (and filter mode if active).
    /// None when no exclusion terms are active.
    pub display_lines: Option<Vec<usize>>,

    /// Whether filter mode is active (hide non-matching lines)
    pub filter_mode: bool,

    /// Index into matching_lines for current match navigation
    pub current_match_index: Option<usize>,

    /// Background search handle (None when idle)
    pub search_handle: Option<SearchHandle>,

    /// Debounce: instant of last query change
    pub last_query_change: Option<std::time::Instant>,
}

impl SearchState {
    pub fn compile(&mut self) {
        // Compile current text field
        if self.query.text.is_empty() {
            self.compiled = None;
            self.compile_error = None;
        } else {
            let pattern = if self.query.is_regex {
                self.query.text.clone()
            } else {
                regex::escape(&self.query.text)
            };
            match RegexBuilder::new(&pattern)
                .case_insensitive(!self.query.case_sensitive)
                .build()
            {
                Ok(re) => {
                    self.compiled = Some(re);
                    self.compile_error = None;
                }
                Err(e) => {
                    self.compiled = None;
                    self.compile_error = Some(e.to_string());
                }
            }
        }

        // Compile committed terms
        self.compiled_terms.clear();
        for term in &self.query.terms {
            if term.is_empty() {
                continue;
            }
            let pattern = if self.query.is_regex {
                term.clone()
            } else {
                regex::escape(term)
            };
            if let Ok(re) = RegexBuilder::new(&pattern)
                .case_insensitive(!self.query.case_sensitive)
                .build()
            {
                self.compiled_terms.push(re);
            }
        }

        // Compile exclusion terms (always literal, same case setting)
        self.compiled_excludes.clear();
        for term in &self.query.exclude_terms {
            if term.is_empty() {
                continue;
            }
            let pattern = if self.query.is_regex {
                term.clone()
            } else {
                regex::escape(term)
            };
            if let Ok(re) = RegexBuilder::new(&pattern)
                .case_insensitive(!self.query.case_sensitive)
                .build()
            {
                self.compiled_excludes.push(re);
            }
        }
    }

    /// Update query text and record when it changed (for debounce).
    pub fn set_query_text(&mut self, text: String) {
        if self.query.text != text {
            self.query.text = text;
            self.last_query_change = Some(std::time::Instant::now());
        }
    }

    /// Returns true if a search should be triggered (debounce elapsed).
    pub fn should_search(&self) -> bool {
        if self.query.text.is_empty() {
            return false;
        }
        self.last_query_change
            .map(|t| t.elapsed() >= std::time::Duration::from_millis(300))
            .unwrap_or(false)
    }

    /// Poll the background search handle for completion.
    /// Returns true if results were updated.
    pub fn poll_search(&mut self, total_lines: usize) -> bool {
        let result = {
            let Some(ref handle) = self.search_handle else { return false };
            handle.receiver.try_recv().ok()
        };

        if let Some(r) = result {
            self.matching_lines = r.matching_lines;
            self.current_match_index = if self.matching_lines.is_empty() {
                None
            } else {
                Some(0)
            };
            if self.filter_mode {
                self.visible_lines = Some(filter::compute_visible_lines(
                    &self.matching_lines,
                    total_lines,
                    self.query.context_lines,
                ));
            }
            self.search_handle = None;
            self.last_query_change = None;
            return true;
        }
        false
    }

    /// Navigate to the next match. Returns the line number to scroll to.
    pub fn next_match(&mut self) -> Option<usize> {
        if self.matching_lines.is_empty() {
            return None;
        }
        let idx = self.current_match_index.map(|i| (i + 1) % self.matching_lines.len()).unwrap_or(0);
        self.current_match_index = Some(idx);
        Some(self.matching_lines[idx])
    }

    /// Navigate to the previous match.
    pub fn prev_match(&mut self) -> Option<usize> {
        if self.matching_lines.is_empty() {
            return None;
        }
        let idx = self.current_match_index
            .map(|i| if i == 0 { self.matching_lines.len() - 1 } else { i - 1 })
            .unwrap_or(0);
        self.current_match_index = Some(idx);
        Some(self.matching_lines[idx])
    }

    pub fn clear(&mut self) {
        self.query.text.clear();
        self.query.terms.clear();
        self.query.exclude_terms.clear();
        self.compiled = None;
        self.compiled_terms.clear();
        self.compiled_excludes.clear();
        self.compile_error = None;
        self.matching_lines.clear();
        self.visible_lines = None;
        self.display_lines = None;
        self.current_match_index = None;
        self.search_handle = None;
        self.last_query_change = None;
    }
}

pub struct SearchHandle {
    pub receiver: Receiver<SearchResult>,
}

pub struct SearchResult {
    pub matching_lines: Vec<usize>,
}

pub fn spawn_search(
    mmap: Arc<MmapFile>,
    index: Arc<FileIndex>,
    query: SearchQuery,
) -> SearchHandle {
    let (tx, rx) = crossbeam_channel::bounded(1);

    std::thread::spawn(move || {
        run_search(&mmap, &index, &query, tx);
    });

    SearchHandle { receiver: rx }
}

fn run_search(
    mmap: &MmapFile,
    index: &FileIndex,
    query: &SearchQuery,
    tx: Sender<SearchResult>,
) {
    // Build all active regexes (current text + committed terms)
    let mut regexes: Vec<Regex> = Vec::new();

    if !query.text.is_empty() {
        let pattern = if query.is_regex {
            query.text.clone()
        } else {
            regex::escape(&query.text)
        };
        if let Ok(re) = RegexBuilder::new(&pattern)
            .case_insensitive(!query.case_sensitive)
            .build()
        {
            regexes.push(re);
        }
    }

    for term in &query.terms {
        if term.is_empty() {
            continue;
        }
        let pattern = if query.is_regex {
            term.clone()
        } else {
            regex::escape(term)
        };
        if let Ok(re) = RegexBuilder::new(&pattern)
            .case_insensitive(!query.case_sensitive)
            .build()
        {
            regexes.push(re);
        }
    }

    if regexes.is_empty() {
        let _ = tx.send(SearchResult { matching_lines: vec![] });
        return;
    }

    let line_count = index.line_count();
    let mut matches = Vec::new();

    for line_num in 0..line_count {
        if let Some(range) = index.line_byte_range(line_num) {
            let text = mmap.line_str(range);
            if regexes.iter().any(|re| re.is_match(&text)) {
                matches.push(line_num);
            }
        }
    }

    let _ = tx.send(SearchResult { matching_lines: matches });
}
