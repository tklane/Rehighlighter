/// Stores byte offsets of every line start in the file.
///
/// For a 1GB file with ~10M lines at 100 bytes avg:
///   10M * 8 bytes (u64) = ~80MB for the index itself.
/// File content stays on disk via mmap.
pub struct FileIndex {
    /// line_starts[i] = byte offset of line i's first character.
    /// line_starts[0] = 0 always.
    /// line_starts[line_count] is a sentinel = file_size.
    line_starts: Vec<u64>,
    file_size: u64,
    pub is_complete: bool,
}

impl FileIndex {
    pub fn new(file_size: u64) -> Self {
        Self {
            line_starts: vec![0],
            file_size,
            is_complete: false,
        }
    }

    /// Append a chunk of new line-start offsets received from the background indexer.
    pub fn extend_with_chunk(&mut self, chunk: Vec<u64>) {
        self.line_starts.extend(chunk);
    }

    /// Called when the background indexer is done.
    /// Adds the sentinel entry = file_size so the last line's byte range is valid.
    pub fn finalize(&mut self) {
        // Remove old sentinel if present, add correct one
        if self.line_starts.last().copied() != Some(self.file_size) {
            self.line_starts.push(self.file_size);
        }
        self.is_complete = true;
    }

    /// Number of complete lines indexed so far.
    pub fn line_count(&self) -> usize {
        if self.line_starts.len() <= 1 {
            return 0;
        }
        // sentinel at end means count = len - 1 when finalized,
        // or len - 1 when not (last entry is a line start, not sentinel)
        if self.is_complete {
            self.line_starts.len() - 1
        } else {
            self.line_starts.len() - 1
        }
    }

    /// Get the byte range [start, end) for line `n` (0-indexed).
    /// Returns None if n is out of range.
    pub fn line_byte_range(&self, n: usize) -> Option<std::ops::Range<u64>> {
        if n >= self.line_count() {
            return None;
        }
        let start = self.line_starts[n];
        let end = self.line_starts[n + 1];
        Some(start..end)
    }

    /// Approximate byte offset of the last indexed line start (for progress %).
    pub fn last_offset(&self) -> u64 {
        self.line_starts.last().copied().unwrap_or(0)
    }

    /// Create a snapshot clone for passing to background search threads.
    pub fn clone_snapshot(&self) -> Self {
        Self {
            line_starts: self.line_starts.clone(),
            file_size: self.file_size,
            is_complete: self.is_complete,
        }
    }

    /// Binary search to find which line contains a given byte offset.
    pub fn line_for_offset(&self, offset: u64) -> usize {
        self.line_starts
            .partition_point(|&s| s <= offset)
            .saturating_sub(1)
    }
}
