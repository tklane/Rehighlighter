/// Compute the set of visible line indices for filter mode.
/// Expands each matching line by `context_lines` in both directions.
pub fn compute_visible_lines(
    matching: &[usize],
    total_lines: usize,
    context_lines: usize,
) -> Vec<usize> {
    use std::collections::BTreeSet;
    let mut visible = BTreeSet::new();
    for &m in matching {
        let start = m.saturating_sub(context_lines);
        let end = (m + context_lines + 1).min(total_lines);
        for i in start..end {
            visible.insert(i);
        }
    }
    visible.into_iter().collect()
}
