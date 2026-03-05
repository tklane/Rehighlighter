use egui::{Color32, RichText, Ui};

use crate::tab::TabState;
use crate::ui::TERM_COLORS;

/// Render the search bar. Returns true if a search should be triggered.
pub fn render_search_bar(ui: &mut Ui, tab: &mut TabState) -> bool {
    let mut trigger_search = false;

    ui.horizontal(|ui| {
        ui.label("Search:");

        // ── Committed term chips ──────────────────────────────────────────────
        let mut remove_term: Option<usize> = None;
        for (i, term) in tab.search.query.terms.iter().enumerate() {
            let bg = TERM_COLORS[i % TERM_COLORS.len()];
            egui::Frame::none()
                .fill(bg)
                .rounding(egui::Rounding::same(3.0))
                .inner_margin(egui::Margin::symmetric(4.0, 1.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.label(RichText::new(term).color(Color32::BLACK).small());
                    if ui.small_button("×").clicked() {
                        remove_term = Some(i);
                    }
                });
        }
        if let Some(i) = remove_term {
            tab.search.query.terms.remove(i);
            if i < tab.search.compiled_terms.len() {
                tab.search.compiled_terms.remove(i);
            }
            trigger_search = true;
        }

        // ── Exclusion chips ───────────────────────────────────────────────────
        let mut remove_exclude: Option<usize> = None;
        for (i, term) in tab.search.query.exclude_terms.iter().enumerate() {
            egui::Frame::none()
                .fill(Color32::from_rgb(160, 40, 40))
                .rounding(egui::Rounding::same(3.0))
                .inner_margin(egui::Margin::symmetric(4.0, 1.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.label(RichText::new(format!("⊘ {term}")).color(Color32::WHITE).small());
                    if ui.small_button("×").clicked() {
                        remove_exclude = Some(i);
                    }
                });
        }
        if let Some(i) = remove_exclude {
            tab.search.query.exclude_terms.remove(i);
            tab.search.compile();
            tab.compute_display_lines();
        }

        // ── Pending clipboard addition ────────────────────────────────────────
        if let Some(ref pending) = tab.pending_search_addition.clone() {
            let preview: String = if pending.chars().count() > 25 {
                format!("{}…", pending.chars().take(25).collect::<String>())
            } else {
                pending.clone()
            };
            if ui
                .button(format!("+ \"{preview}\""))
                .on_hover_text("Add copied text as a search term")
                .clicked()
            {
                tab.search.query.terms.push(pending.clone());
                tab.pending_search_addition = None;
                trigger_search = true;
            }
            if ui.small_button("✕").on_hover_text("Dismiss").clicked() {
                tab.pending_search_addition = None;
            }
        }

        // ── Text input ────────────────────────────────────────────────────────
        let response = ui.add(
            egui::TextEdit::singleline(&mut tab.search.query.text)
                .hint_text("Search… (Enter to add term)")
                .desired_width(250.0),
        );

        if response.changed() {
            if tab.search.query.text.is_empty() && !tab.search.query.terms.is_empty() {
                // Text cleared; immediately re-search with just the committed terms
                trigger_search = true;
            } else {
                tab.search.last_query_change = Some(std::time::Instant::now());
            }
        }

        // Commit current text as a new term on Enter
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let text = tab.search.query.text.trim().to_string();
            if !text.is_empty() {
                tab.search.query.text.clear();
                tab.search.query.terms.push(text);
                tab.search.last_query_change = None;
                trigger_search = true;
            }
        }

        // ── Regex toggle ──────────────────────────────────────────────────────
        if ui
            .selectable_label(tab.search.query.is_regex, "regex")
            .clicked()
        {
            tab.search.query.is_regex = !tab.search.query.is_regex;
            if !tab.search.query.text.is_empty() || !tab.search.query.terms.is_empty() {
                trigger_search = true;
            }
        }

        // ── Case sensitive toggle ─────────────────────────────────────────────
        if ui
            .selectable_label(tab.search.query.case_sensitive, "Aa")
            .on_hover_text("Case sensitive")
            .clicked()
        {
            tab.search.query.case_sensitive = !tab.search.query.case_sensitive;
            if !tab.search.query.text.is_empty() || !tab.search.query.terms.is_empty() {
                trigger_search = true;
            }
        }

        // ── Filter mode toggle ────────────────────────────────────────────────
        if ui
            .selectable_label(tab.search.filter_mode, "Filter")
            .on_hover_text("Hide non-matching lines")
            .clicked()
        {
            tab.search.filter_mode = !tab.search.filter_mode;
            if tab.search.filter_mode && !tab.search.matching_lines.is_empty() {
                let total = tab.index.line_count();
                tab.search.visible_lines = Some(
                    crate::search::filter::compute_visible_lines(
                        &tab.search.matching_lines,
                        total,
                        tab.search.query.context_lines,
                    ),
                );
            } else {
                tab.search.visible_lines = None;
            }
        }

        // ── Context lines spinner ─────────────────────────────────────────────
        if tab.search.filter_mode {
            ui.label("±");
            let mut ctx_lines = tab.search.query.context_lines as i32;
            if ui
                .add(egui::DragValue::new(&mut ctx_lines).range(0..=50).speed(1.0))
                .changed()
            {
                tab.search.query.context_lines = ctx_lines.max(0) as usize;
                if !tab.search.matching_lines.is_empty() {
                    let total = tab.index.line_count();
                    tab.search.visible_lines = Some(
                        crate::search::filter::compute_visible_lines(
                            &tab.search.matching_lines,
                            total,
                            tab.search.query.context_lines,
                        ),
                    );
                }
            }
        }

        // ── Navigation buttons ────────────────────────────────────────────────
        if !tab.search.matching_lines.is_empty() {
            if ui.button("▲").clicked() {
                if let Some(line) = tab.search.prev_match() {
                    tab.scroll_to_row = Some(line_to_row(tab, line));
                }
            }
            if ui.button("▼").clicked() {
                if let Some(line) = tab.search.next_match() {
                    tab.scroll_to_row = Some(line_to_row(tab, line));
                }
            }

            let idx = tab.search.current_match_index.map(|i| i + 1).unwrap_or(0);
            let total = tab.search.matching_lines.len();
            ui.label(format!("{idx}/{total}"));
        }

        // ── Clear button ──────────────────────────────────────────────────────
        let has_anything = !tab.search.query.text.is_empty()
            || !tab.search.query.terms.is_empty()
            || !tab.search.query.exclude_terms.is_empty();
        if has_anything && ui.button("×").clicked() {
            tab.search.clear();
            // display_lines is already cleared by clear(); no recompute needed
        }

        // ── Compile error ─────────────────────────────────────────────────────
        if let Some(ref err) = tab.search.compile_error.clone() {
            ui.label(RichText::new(format!("⚠ {err}")).color(Color32::RED).small());
        }

        // ── Histogram toggle (far right) ──────────────────────────────────────
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .selectable_label(tab.show_histogram, "📊")
                .on_hover_text("Toggle time histogram")
                .clicked()
            {
                tab.show_histogram = !tab.show_histogram;
            }
        });
    });

    trigger_search
}

/// Convert a file line number to a logical row index (accounts for filter mode).
fn line_to_row(tab: &TabState, line: usize) -> usize {
    if tab.search.filter_mode {
        if let Some(ref visible) = tab.search.visible_lines {
            return visible.partition_point(|&v| v < line);
        }
    }
    line
}
