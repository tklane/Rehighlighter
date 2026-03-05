use memmap2::Mmap;
use std::{fs::File, path::Path};

pub struct MmapFile {
    _file: File,
    mmap: Mmap,
}

impl MmapFile {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let file = File::open(path)?;
        // SAFETY: File is read-only. We do not modify it while mapped.
        let mmap = unsafe { Mmap::map(&file)? };
        Ok(Self { _file: file, mmap })
    }

    /// Get a line's content as raw bytes given a byte range.
    /// Strips trailing \r\n or \n.
    pub fn line_bytes(&self, range: std::ops::Range<u64>) -> &[u8] {
        let start = range.start as usize;
        let end = (range.end as usize).min(self.mmap.len());
        let raw = &self.mmap[start..end];
        raw.strip_suffix(b"\r\n")
            .or_else(|| raw.strip_suffix(b"\n"))
            .unwrap_or(raw)
    }

    /// Decode a line to a string. Lossy UTF-8 so binary/mixed files don't crash.
    pub fn line_str(&self, range: std::ops::Range<u64>) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(self.line_bytes(range))
    }

    pub fn len(&self) -> u64 {
        self.mmap.len() as u64
    }

    pub fn is_empty(&self) -> bool {
        self.mmap.is_empty()
    }

    /// Access the full file as a byte slice (zero-copy, tied to mmap lifetime).
    pub fn as_bytes(&self) -> &[u8] {
        &self.mmap
    }
}
