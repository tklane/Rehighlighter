use std::sync::Arc;

use crossbeam_channel::Receiver;

use crate::indexer::FileIndex;

/// Per-slot normalized line lengths for the overview panel wide mode.
pub struct OverviewCache {
    pub num_slots: usize,
    /// Normalized line length per slot, in [0.0, 1.0].
    pub lengths: Vec<f32>,
    pub total_lines: usize,
}

pub struct OverviewCacheHandle {
    pub receiver: Receiver<OverviewCache>,
}

/// Spawn a background thread that computes the overview cache from the index.
pub fn spawn_overview_cache(index: Arc<FileIndex>) -> OverviewCacheHandle {
    let (tx, rx) = crossbeam_channel::bounded(1);

    std::thread::spawn(move || {
        let total_lines = index.line_count();
        if total_lines == 0 {
            let _ = tx.send(OverviewCache {
                num_slots: 0,
                lengths: vec![],
                total_lines: 0,
            });
            return;
        }

        // Cap slots so we don't do excessive work
        let num_slots = total_lines.min(2000);
        let mut slot_sum = vec![0u64; num_slots];
        let mut slot_count = vec![0u32; num_slots];

        for line in 0..total_lines {
            if let Some(range) = index.line_byte_range(line) {
                let len = range.end.saturating_sub(range.start) as u64;
                let slot = (line * num_slots / total_lines).min(num_slots - 1);
                slot_sum[slot] += len;
                slot_count[slot] += 1;
            }
        }

        // Compute average per slot as f32
        let mut lengths: Vec<f32> = slot_sum
            .iter()
            .zip(slot_count.iter())
            .map(|(&s, &c)| if c > 0 { s as f32 / c as f32 } else { 0.0 })
            .collect();

        // Normalize to [0, 1]
        let max_len = lengths.iter().copied().fold(0.0_f32, f32::max);
        if max_len > 0.0 {
            for v in &mut lengths {
                *v /= max_len;
            }
        }

        let _ = tx.send(OverviewCache {
            num_slots,
            lengths,
            total_lines,
        });
    });

    OverviewCacheHandle { receiver: rx }
}
