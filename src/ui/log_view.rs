use std::cell::RefCell;

use egui::{Color32, Context, FontId, TextFormat, Ui};
use egui_extras::{Column, TableBuilder};
use regex::Regex;

use crate::tab::TabState;
use crate::ui::TERM_COLORS;

/// Maximum characters to display per line in the main view.
const MAX_DISPLAY_CHARS: usize = 4096;

pub fn render_log_view(ui: &mut Ui, tab: &mut TabState, _ctx: &Context) {
    let available_height = ui.available_height();

    let filter_mode = tab.search.filter_mode;
    let total_rows = if let Some(ref dl) = tab.search.display_lines {
        dl.len()
    } else if filter_mode {
        tab.search.visible_lines.as_ref().map(|v| v.len()).unwrap_or(0)
    } else {
        tab.index.line_count()
    };

    if total_rows == 0 {
        ui.centered_and_justified(|ui| match &tab.status {
            crate::tab::TabStatus::Indexing { progress_pct } => {
                ui.label(format!("Indexing… {:.0}%", progress_pct));
            }
            crate::tab::TabStatus::Error(e) => {
                ui.label(format!("Error: {e}"));
            }
            crate::tab::TabStatus::Ready => {
                ui.label("No lines to display");
            }
        });
        return;
    }

    let text_style = egui::TextStyle::Monospace;
    let row_height = ui.text_style_height(&text_style) + 4.0;

    // Consume mutable things before the immutable borrow of tab.index
    let scroll_to = tab.scroll_to_row.take();

    // Clone what the closure needs (avoids lifetime conflicts with tab.index borrow)
    let mmap = tab.mmap.clone();
    let matching_lines = tab.search.matching_lines.clone();
    let current_match_idx = tab.search.current_match_index;
    let compiled = tab.search.compiled.clone();
    let compiled_terms = tab.search.compiled_terms.clone();
    let visible_lines: Option<Vec<usize>> = if filter_mode {
        tab.search.visible_lines.clone()
    } else {
        None
    };
    let display_lines: Option<Vec<usize>> = tab.search.display_lines.clone();

    // Build the (regex, highlight-color) list for all active terms.
    // Committed terms first, then the current text field (if any).
    let highlight_terms: Vec<(Regex, Color32)> = {
        let mut v: Vec<(Regex, Color32)> = compiled_terms
            .iter()
            .enumerate()
            .map(|(i, re)| (re.clone(), TERM_COLORS[i % TERM_COLORS.len()]))
            .collect();
        if let Some(ref re) = compiled {
            let color = TERM_COLORS[compiled_terms.len() % TERM_COLORS.len()];
            v.push((re.clone(), color));
        }
        v
    };

    // Collect side-effects here; applied after the table block ends
    let clicked_line: RefCell<Option<usize>> = RefCell::new(None);
    let double_clicked_word: RefCell<Option<String>> = RefCell::new(None);
    let exclude_word: RefCell<Option<String>> = RefCell::new(None);
    let search_word: RefCell<Option<String>> = RefCell::new(None);
    // Temp-data keys: ctx_word_id holds the initial token; ctx_edit_id holds the live edit buffer
    let ctx_word_id = egui::Id::new("log_view_ctx_word");
    let ctx_edit_id = egui::Id::new("log_view_ctx_edit");

    // Horizontal scroll wraps the table; vertical scroll is handled by TableBuilder internally.
    egui::ScrollArea::horizontal()
        .id_salt("log_view_h")
        .show(ui, |ui| {
        let tab_index = &tab.index;

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(60.0))
            .column(Column::auto())
            .min_scrolled_height(available_height)
            .max_scroll_height(available_height);

        if let Some(row) = scroll_to {
            table = table.scroll_to_row(row, Some(egui::Align::Center));
        }

        table.body(|body| {
            body.rows(row_height, total_rows, |mut row| {
                let row_idx = row.index();

                let line_num = if let Some(ref dl) = display_lines {
                    dl[row_idx]
                } else if filter_mode {
                    visible_lines.as_ref().map(|v| v[row_idx]).unwrap_or(row_idx)
                } else {
                    row_idx
                };

                let Some(byte_range) = tab_index.line_byte_range(line_num) else {
                    return;
                };

                let raw_content = mmap.line_str(byte_range);
                let content: &str = if raw_content.len() > MAX_DISPLAY_CHARS {
                    &raw_content[..MAX_DISPLAY_CHARS]
                } else {
                    &raw_content
                };
                let truncated = raw_content.len() > MAX_DISPLAY_CHARS;

                let is_match = matching_lines.binary_search(&line_num).is_ok();
                let is_current = current_match_idx
                    .and_then(|i| matching_lines.get(i))
                    .map(|&m| m == line_num)
                    .unwrap_or(false);

                // Line number gutter
                row.col(|ui| {
                    let color = if is_match {
                        Color32::from_rgb(255, 200, 50)
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    ui.colored_label(color, format!("{:>6}", line_num + 1));
                });

                // Content column
                row.col(|ui| {
                    if is_current {
                        ui.painter().rect_filled(
                            ui.max_rect(),
                            0.0,
                            Color32::from_rgba_premultiplied(255, 165, 0, 60),
                        );
                    }

                    let job = build_highlighted_job(content, &highlight_terms, ui, truncated);
                    let resp = ui.add(egui::Label::new(job).selectable(true));

                    // Right-click: capture token at cursor into egui temp-data.
                    // Also initialise the editable buffer with that token.
                    if resp.secondary_clicked() {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            let char_width = ui.fonts(|f| {
                                f.glyph_width(&FontId::monospace(13.0), 'a')
                            });
                            let rel_x = (pos.x - resp.rect.left()).max(0.0);
                            let char_idx = (rel_x / char_width) as usize;
                            let word = word_at(content, char_idx);
                            let initial = word.clone().unwrap_or_default();
                            ui.ctx().data_mut(|d| {
                                d.insert_temp::<Option<String>>(ctx_word_id, word);
                                d.insert_temp::<String>(ctx_edit_id, initial);
                            });
                        }
                    }

                    resp.context_menu(|ui| {
                        // Load (and persist) the editable buffer
                        let mut buf: String = ui
                            .ctx()
                            .data(|d| d.get_temp::<String>(ctx_edit_id))
                            .unwrap_or_default();

                        ui.label("Search / exclude:");
                        let edit_resp = ui.add(
                            egui::TextEdit::singleline(&mut buf)
                                .desired_width(220.0)
                                .font(egui::TextStyle::Monospace),
                        );
                        // Request focus on the first frame so the user can type immediately
                        if edit_resp.gained_focus() || ui.ctx().data(|d| d.get_temp::<bool>(ctx_edit_id.with("focused"))).is_none() {
                            edit_resp.request_focus();
                            ui.ctx().data_mut(|d| d.insert_temp::<bool>(ctx_edit_id.with("focused"), true));
                        }
                        // Persist any edits back to temp-data
                        ui.ctx().data_mut(|d| d.insert_temp::<String>(ctx_edit_id, buf.clone()));

                        ui.horizontal(|ui| {
                            let trimmed = buf.trim().to_string();
                            let enabled = !trimmed.is_empty();
                            let do_search = ui.add_enabled(enabled, egui::Button::new("🔍 Search")).clicked();
                            let do_exclude = ui.add_enabled(enabled, egui::Button::new("⊘ Exclude")).clicked();
                            if do_search || do_exclude {
                                if do_search {
                                    *search_word.borrow_mut() = Some(trimmed);
                                } else {
                                    *exclude_word.borrow_mut() = Some(trimmed);
                                }
                                ui.ctx().data_mut(|d| {
                                    d.remove::<Option<String>>(ctx_word_id);
                                    d.remove::<String>(ctx_edit_id);
                                    d.remove::<bool>(ctx_edit_id.with("focused"));
                                });
                                ui.close_menu();
                            }
                        });
                    });

                    // Double-click: add word under cursor as a search term.
                    // (Single copy with ⌘C still works for multi-word phrases.)
                    if resp.double_clicked() {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            let char_width = ui.fonts(|f| {
                                f.glyph_width(&FontId::monospace(13.0), 'a')
                            });
                            let rel_x = (pos.x - resp.rect.left()).max(0.0);
                            let char_idx = (rel_x / char_width) as usize;
                            if let Some(word) = word_at(content, char_idx) {
                                *double_clicked_word.borrow_mut() = Some(word);
                            }
                        }
                    } else if resp.clicked() {
                        *clicked_line.borrow_mut() = Some(line_num);
                    }

                    if truncated {
                        resp.on_hover_text("Line truncated — click to view full line");
                    }
                });
            });
        });
    }); // horizontal scroll area ends here

    // Apply single-click: open detail panel
    if let Some(line) = clicked_line.into_inner() {
        tab.detail_line = Some(line);
        tab.detail_open = true;
    }

    // Apply double-click: add word under cursor as a search term
    if let Some(word) = double_clicked_word.into_inner() {
        tab.search.query.terms.push(word);
        tab.trigger_search();
    }

    // Apply right-click search: add word as a search term
    if let Some(word) = search_word.into_inner() {
        tab.search.query.terms.push(word);
        tab.trigger_search();
    }

    // Apply right-click exclusion: hide lines matching the word
    if let Some(word) = exclude_word.into_inner() {
        tab.search.query.exclude_terms.push(word);
        tab.search.compile();
        tab.compute_display_lines();
    }
}

/// Extract the contiguous non-whitespace token at `char_idx`.
/// Returns `None` if the character is whitespace or the index is out of bounds.
/// Expands left and right stopping only at whitespace, so tokens like
/// "192.168.1.1", "/path/to/file", and "ERROR:" are selected whole.
fn word_at(text: &str, char_idx: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || char_idx >= chars.len() {
        return None;
    }
    if chars[char_idx].is_whitespace() {
        return None;
    }
    // Expand left
    let mut start = char_idx;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    // Expand right
    let mut end = char_idx + 1;
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }
    let word: String = chars[start..end].iter().collect();
    if word.is_empty() { None } else { Some(word) }
}

fn build_highlighted_job(
    text: &str,
    terms: &[(Regex, Color32)],
    ui: &Ui,
    truncated: bool,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();

    let default_fmt = TextFormat {
        font_id: FontId::monospace(13.0),
        color: ui.visuals().text_color(),
        ..Default::default()
    };

    let trunc_fmt = TextFormat {
        font_id: FontId::monospace(11.0),
        color: ui.visuals().weak_text_color(),
        ..Default::default()
    };

    if terms.is_empty() {
        job.append(text, 0.0, default_fmt);
        if truncated {
            job.append(" …", 0.0, trunc_fmt);
        }
        return job;
    }

    // Collect all (start, end, color) spans from all regexes
    let mut spans: Vec<(usize, usize, Color32)> = Vec::new();
    for (re, color) in terms {
        for m in re.find_iter(text) {
            spans.push((m.start(), m.end(), *color));
        }
    }
    // Sort by start position; on tie, prefer the first-listed term (lower index)
    spans.sort_by_key(|&(start, _, _)| start);

    let mut last_end = 0usize;
    for (start, end, bg_color) in spans {
        if start < last_end {
            // Overlapping span — skip (first match wins)
            continue;
        }
        if start > last_end {
            job.append(&text[last_end..start], 0.0, default_fmt.clone());
        }
        let highlight_fmt = TextFormat {
            font_id: FontId::monospace(13.0),
            color: Color32::BLACK,
            background: bg_color,
            ..Default::default()
        };
        job.append(&text[start..end], 0.0, highlight_fmt);
        last_end = end;
    }
    if last_end < text.len() {
        job.append(&text[last_end..], 0.0, default_fmt);
    }
    if truncated {
        job.append(" …", 0.0, trunc_fmt);
    }

    job
}
