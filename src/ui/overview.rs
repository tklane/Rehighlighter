use egui::{Color32, Rect, Sense, Stroke, Ui};

use crate::tab::TabState;
use crate::ui::TERM_COLORS;

const WIDE_THRESHOLD: f32 = 60.0;

/// Renders the overview panel.
/// - Narrow mode (< 60px): tick marks for search matches.
/// - Wide mode (≥ 60px): line-length bars with match-position highlighting.
/// Returns Some(row) if the user clicked to navigate.
pub fn render_overview(ui: &mut Ui, tab: &TabState) -> Option<usize> {
    let size = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    if !ui.is_rect_visible(rect) {
        return None;
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

    let total_lines = if let Some(ref dl) = tab.search.display_lines {
        dl.len()
    } else if tab.search.filter_mode {
        tab.search
            .visible_lines
            .as_ref()
            .map(|v| v.len())
            .unwrap_or(0)
    } else {
        tab.index.line_count()
    };

    if total_lines == 0 {
        return click_result(&response, rect, 0);
    }

    let wide = rect.width() >= WIDE_THRESHOLD;

    if wide {
        render_wide(ui, &painter, rect, tab, total_lines);
    } else {
        render_narrow(&painter, rect, tab, total_lines);
    }

    click_result(&response, rect, total_lines)
}

// ── Narrow mode ──────────────────────────────────────────────────────────────

fn render_narrow(
    painter: &egui::Painter,
    rect: Rect,
    tab: &TabState,
    total_lines: usize,
) {
    let matching_lines = &tab.search.matching_lines;
    if matching_lines.is_empty() {
        return;
    }

    let height = rect.height();
    let current_line = tab
        .search
        .current_match_index
        .and_then(|i| matching_lines.get(i))
        .copied();

    // One pass per pixel row — O(height) regex calls instead of O(matching_lines).
    let pixel_rows = height as usize;
    for py in 0..pixel_rows {
        let frac = py as f32 / height;
        let line = ((frac * total_lines as f32) as usize).min(total_lines.saturating_sub(1));

        let match_idx = matching_lines.partition_point(|&m| m < line);
        if matching_lines.get(match_idx).copied() != Some(line) {
            continue;
        }

        let y = rect.top() + py as f32 + 0.5;
        let color = if Some(line) == current_line {
            Color32::from_rgb(255, 130, 0)
        } else {
            first_match_color(tab, line).unwrap_or(Color32::from_rgb(255, 220, 50))
        };
        painter.hline(rect.x_range(), y, Stroke::new(1.5, color));
    }
}

// ── Wide mode ─────────────────────────────────────────────────────────────────

fn render_wide(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    tab: &TabState,
    total_lines: usize,
) {
    let height = rect.height();
    let width = rect.width();
    let matching_lines = &tab.search.matching_lines;

    let bar_color = Color32::BLACK;
    let bg = ui.visuals().extreme_bg_color;

    let pixel_rows = height as usize;
    for py in 0..pixel_rows {
        let y = rect.top() + py as f32 + 0.5;
        let frac = py as f32 / height;
        let line = ((frac * total_lines as f32) as usize).min(total_lines.saturating_sub(1));

        // Determine bar width from cache
        let bar_frac = tab
            .overview_cache
            .as_ref()
            .and_then(|cache| {
                if cache.num_slots == 0 {
                    return None;
                }
                let slot = (line * cache.num_slots / total_lines).min(cache.num_slots - 1);
                cache.lengths.get(slot).copied()
            })
            .unwrap_or(0.0);

        if bar_frac <= 0.0 {
            continue;
        }

        let bar_right = rect.left() + bar_frac * width;

        // Check if this line has a search match
        let match_idx = matching_lines.partition_point(|&m| m < line);
        let has_match = matching_lines.get(match_idx).copied() == Some(line);

        if !has_match {
            painter.hline(rect.left()..=bar_right, y, Stroke::new(1.0, bar_color));
        } else {
            // get_match_span now returns the per-term color too
            let span = get_match_span(tab, line);

            if let Some((start_frac, end_frac, match_color)) = span {
                let match_left = rect.left() + start_frac * bar_frac * width;
                let match_right = rect.left() + end_frac * bar_frac * width;

                if match_left > rect.left() {
                    painter.hline(rect.left()..=match_left, y, Stroke::new(1.0, bar_color));
                }
                painter.hline(match_left..=match_right, y, Stroke::new(1.0, match_color));
                if match_right < bar_right {
                    painter.hline(match_right..=bar_right, y, Stroke::new(1.0, bar_color));
                }
            } else {
                // No span — paint whole bar in first matching term's color
                let fallback = first_match_color(tab, line).unwrap_or(TERM_COLORS[0]);
                painter.hline(rect.left()..=bar_right, y, Stroke::new(1.0, fallback));
            }
        }

        if bar_right < rect.right() {
            painter.hline(bar_right..=rect.right(), y, Stroke::new(1.0, bg));
        }
    }
}

/// Returns the color of the first committed term that matches this line,
/// then falls back to the current (text-field) term.
/// Term order mirrors the search bar chip order.
fn first_match_color(tab: &TabState, line: usize) -> Option<Color32> {
    let range = tab.index.line_byte_range(line)?;
    let bytes = tab.mmap.line_bytes(range);
    let text = std::str::from_utf8(bytes).ok()?;

    for (i, re) in tab.search.compiled_terms.iter().enumerate() {
        if re.is_match(text) {
            return Some(TERM_COLORS[i % TERM_COLORS.len()]);
        }
    }
    if let Some(ref re) = tab.search.compiled {
        if re.is_match(text) {
            return Some(TERM_COLORS[tab.search.compiled_terms.len() % TERM_COLORS.len()]);
        }
    }
    None
}

/// Get the match byte-span within a line as fractions of line length,
/// plus the color of the matching term.
fn get_match_span(tab: &TabState, line: usize) -> Option<(f32, f32, Color32)> {
    let range = tab.index.line_byte_range(line)?;
    let line_len = range.end.saturating_sub(range.start);
    if line_len == 0 {
        return None;
    }
    let bytes = tab.mmap.line_bytes(range);
    let text = std::str::from_utf8(bytes).ok()?;

    for (i, re) in tab.search.compiled_terms.iter().enumerate() {
        if let Some(m) = re.find(text) {
            let color = TERM_COLORS[i % TERM_COLORS.len()];
            return Some((
                (m.start() as f32 / line_len as f32).min(1.0),
                (m.end() as f32 / line_len as f32).min(1.0),
                color,
            ));
        }
    }
    if let Some(ref re) = tab.search.compiled {
        if let Some(m) = re.find(text) {
            let color = TERM_COLORS[tab.search.compiled_terms.len() % TERM_COLORS.len()];
            return Some((
                (m.start() as f32 / line_len as f32).min(1.0),
                (m.end() as f32 / line_len as f32).min(1.0),
                color,
            ));
        }
    }
    None
}

// ── Shared click handling ─────────────────────────────────────────────────────

fn click_result(
    response: &egui::Response,
    rect: Rect,
    total_lines: usize,
) -> Option<usize> {
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let height = rect.height();
            let frac = ((pos.y - rect.top()) / height).clamp(0.0, 1.0);
            let row = (frac * total_lines as f32) as usize;
            return Some(row.min(total_lines.saturating_sub(1)));
        }
    }
    None
}
