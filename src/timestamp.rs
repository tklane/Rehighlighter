use std::sync::Arc;

use crossbeam_channel::Receiver;
use regex::Regex;

use crate::indexer::{FileIndex, MmapFile};

pub const NO_BUCKET: i32 = -1;

#[derive(Debug, Clone, Default)]
pub struct HistogramData {
    pub buckets: Vec<usize>,
    pub bucket_secs: i64,
    pub start_ts: i64,
    pub max_count: usize,
    pub total_with_ts: usize,
    /// Per-line bucket index. Indexed by line number. NO_BUCKET if no timestamp.
    pub line_to_bucket: Vec<i32>,
    /// Raw (line_num, unix_ts) pairs for rebinning without re-scanning.
    pub pairs: Vec<(usize, i64)>,
    /// Total number of lines (needed for line_to_bucket sizing on rebin).
    pub line_count: usize,
}

impl HistogramData {
    pub fn bucket_ts(&self, idx: usize) -> i64 {
        self.start_ts + idx as i64 * self.bucket_secs
    }

    /// Compute how many matching lines fall into each bucket.
    pub fn compute_match_counts(&self, matching_lines: &[usize]) -> Vec<usize> {
        let mut counts = vec![0usize; self.buckets.len()];
        for &line in matching_lines {
            if let Some(&bi) = self.line_to_bucket.get(line) {
                if bi >= 0 {
                    counts[bi as usize] += 1;
                }
            }
        }
        counts
    }
}

pub struct HistogramHandle {
    pub receiver: Receiver<HistogramData>,
}

pub fn spawn_histogram(mmap: Arc<MmapFile>, index: Arc<FileIndex>) -> HistogramHandle {
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        if let Some(data) = compute_histogram(&mmap, &index) {
            let _ = tx.send(data);
        }
    });
    HistogramHandle { receiver: rx }
}

// ── Timestamp format detection ────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum TsFmt {
    Iso,
    Syslog,
    Apache,
}

struct TsParser {
    re: Regex,
    fmt: TsFmt,
    syslog_year: i32,
}

impl TsParser {
    fn iso() -> Option<Self> {
        Some(Self {
            re: Regex::new(r"\b(\d{4})[-/](\d{2})[-/](\d{2})[T ](\d{2}):(\d{2}):(\d{2})").ok()?,
            fmt: TsFmt::Iso,
            syslog_year: 0,
        })
    }

    fn syslog(year: i32) -> Option<Self> {
        Some(Self {
            re: Regex::new(r"\b([A-Za-z]{3})\s{1,2}(\d{1,2})\s+(\d{2}):(\d{2}):(\d{2})").ok()?,
            fmt: TsFmt::Syslog,
            syslog_year: year,
        })
    }

    fn apache() -> Option<Self> {
        Some(Self {
            re: Regex::new(r"\[(\d{2})/([A-Za-z]{3})/(\d{4}):(\d{2}):(\d{2}):(\d{2})").ok()?,
            fmt: TsFmt::Apache,
            syslog_year: 0,
        })
    }

    fn parse(&self, line: &str) -> Option<i64> {
        let c = self.re.captures(line)?;
        match self.fmt {
            TsFmt::Iso => {
                let y: i32 = c[1].parse().ok()?;
                let mo: u32 = c[2].parse().ok()?;
                let d: u32 = c[3].parse().ok()?;
                let h: u32 = c[4].parse().ok()?;
                let mi: u32 = c[5].parse().ok()?;
                let s: u32 = c[6].parse().ok()?;
                Some(to_unix_ts(y, mo, d, h, mi, s))
            }
            TsFmt::Syslog => {
                let mo = month_abbr(&c[1])?;
                let d: u32 = c[2].parse().ok()?;
                let h: u32 = c[3].parse().ok()?;
                let mi: u32 = c[4].parse().ok()?;
                let s: u32 = c[5].parse().ok()?;
                Some(to_unix_ts(self.syslog_year, mo, d, h, mi, s))
            }
            TsFmt::Apache => {
                let d: u32 = c[1].parse().ok()?;
                let mo = month_abbr(&c[2])?;
                let y: i32 = c[3].parse().ok()?;
                let h: u32 = c[4].parse().ok()?;
                let mi: u32 = c[5].parse().ok()?;
                let s: u32 = c[6].parse().ok()?;
                Some(to_unix_ts(y, mo, d, h, mi, s))
            }
        }
    }
}

fn detect_parser(mmap: &MmapFile, index: &FileIndex) -> Option<TsParser> {
    let sample = index.line_count().min(50);

    let iso_re = Regex::new(r"\b\d{4}[-/]\d{2}[-/]\d{2}[T ]\d{2}:\d{2}:\d{2}").ok()?;
    let syslog_re = Regex::new(r"\b[A-Za-z]{3}\s{1,2}\d{1,2}\s+\d{2}:\d{2}:\d{2}").ok()?;
    let apache_re = Regex::new(r"\[\d{2}/[A-Za-z]{3}/\d{4}:\d{2}:\d{2}:\d{2}").ok()?;

    let mut iso = 0u32;
    let mut syslog = 0u32;
    let mut apache = 0u32;

    for i in 0..sample {
        if let Some(r) = index.line_byte_range(i) {
            let line = mmap.line_str(r);
            if iso_re.is_match(&line) {
                iso += 1;
            } else if apache_re.is_match(&line) {
                apache += 1;
            } else if syslog_re.is_match(&line) {
                syslog += 1;
            }
        }
    }

    let threshold = (sample as u32) / 5; // 20%
    let best = iso.max(apache).max(syslog);
    if best < threshold.max(2) {
        return None;
    }

    let year = approximate_current_year();
    if iso >= apache && iso >= syslog {
        TsParser::iso()
    } else if apache >= syslog {
        TsParser::apache()
    } else {
        TsParser::syslog(year)
    }
}

// ── Main computation ──────────────────────────────────────────────────────────

/// Re-bin an existing set of (line_num, unix_ts) pairs with a new bucket size.
pub fn rebin_from_pairs(pairs: &[(usize, i64)], bucket_secs: i64, line_count: usize) -> HistogramData {
    if pairs.is_empty() {
        return HistogramData { line_count, ..Default::default() };
    }
    let min_ts = pairs.iter().map(|&(_, t)| t).min().unwrap();
    let max_ts = pairs.iter().map(|&(_, t)| t).max().unwrap();
    let num_buckets = (((max_ts - min_ts) / bucket_secs) + 2).min(2000) as usize;
    let mut buckets = vec![0usize; num_buckets];
    let mut line_to_bucket = vec![NO_BUCKET; line_count];

    for &(ln, ts) in pairs {
        let bi = (((ts - min_ts) / bucket_secs) as usize).min(num_buckets - 1);
        buckets[bi] += 1;
        if ln < line_count {
            line_to_bucket[ln] = bi as i32;
        }
    }

    let max_count = buckets.iter().copied().max().unwrap_or(0);

    HistogramData {
        buckets,
        bucket_secs,
        start_ts: min_ts,
        max_count,
        total_with_ts: pairs.len(),
        line_to_bucket,
        pairs: pairs.to_vec(),
        line_count,
    }
}

/// Auto-select bucket_secs from the data's time range.
pub fn auto_bucket_secs(pairs: &[(usize, i64)]) -> i64 {
    if pairs.len() < 2 {
        return 60;
    }
    let min_ts = pairs.iter().map(|&(_, t)| t).min().unwrap();
    let max_ts = pairs.iter().map(|&(_, t)| t).max().unwrap();
    let range = (max_ts - min_ts).max(1);
    if range <= 7_200 {
        60
    } else if range <= 172_800 {
        3_600
    } else if range <= 7_776_000 {
        86_400
    } else if range <= 63_072_000 {
        604_800
    } else {
        2_592_000
    }
}

fn compute_histogram(mmap: &MmapFile, index: &FileIndex) -> Option<HistogramData> {
    let parser = detect_parser(mmap, index)?;
    let line_count = index.line_count();

    // Collect (line_num, unix_ts) pairs
    let mut pairs: Vec<(usize, i64)> = Vec::new();
    for ln in 0..line_count {
        if let Some(r) = index.line_byte_range(ln) {
            let line = mmap.line_str(r);
            if let Some(ts) = parser.parse(&line) {
                pairs.push((ln, ts));
            }
        }
    }

    if pairs.len() < 10 {
        return None;
    }

    let bucket_secs = auto_bucket_secs(&pairs);
    Some(rebin_from_pairs(&pairs, bucket_secs, line_count))
}

// ── Date math (Howard Hinnant algorithm, no external deps) ───────────────────

pub fn to_unix_ts(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let m = month as i64;
    let d = day as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) as u64 / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe as i64 - 719_468;
    days * 86_400 + hour as i64 * 3_600 + min as i64 * 60 + sec as i64
}

/// Returns (year, month, day, hour, minute) from a unix timestamp.
pub fn unix_to_ymdh(ts: i64) -> (i32, u32, u32, u32, u32) {
    let secs_in_day = ts.rem_euclid(86_400) as u32;
    let days = (ts - secs_in_day as i64) / 86_400;
    let hour = secs_in_day / 3_600;
    let min = (secs_in_day % 3_600) / 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y } as i32;

    (y, m, d, hour, min)
}

pub fn format_bucket_label(ts: i64, bucket_secs: i64) -> String {
    let (year, month, day, hour, min) = unix_to_ymdh(ts);
    if bucket_secs < 3_600 {
        format!("{hour:02}:{min:02}")
    } else if bucket_secs < 86_400 {
        format!("{month:02}/{day:02} {hour:02}h")
    } else if bucket_secs < 2_592_000 {
        format!("{year:04}-{month:02}-{day:02}")
    } else {
        format!("{year:04}-{month:02}")
    }
}

fn month_abbr(s: &str) -> Option<u32> {
    match s {
        "Jan" | "jan" => Some(1),
        "Feb" | "feb" => Some(2),
        "Mar" | "mar" => Some(3),
        "Apr" | "apr" => Some(4),
        "May" | "may" => Some(5),
        "Jun" | "jun" => Some(6),
        "Jul" | "jul" => Some(7),
        "Aug" | "aug" => Some(8),
        "Sep" | "sep" => Some(9),
        "Oct" | "oct" => Some(10),
        "Nov" | "nov" => Some(11),
        "Dec" | "dec" => Some(12),
        _ => None,
    }
}

fn approximate_current_year() -> i32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    1970 + (secs / 31_557_600) as i32
}
