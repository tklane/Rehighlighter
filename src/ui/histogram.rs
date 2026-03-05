use egui::{Color32, FontId, Pos2, Rect, Sense, Ui, Vec2};

use crate::{tab::TabState, timestamp::format_bucket_label};

// (label, bucket_secs). 0 = Auto.
const GRANULARITIES: &[(&str, i64)] = &[
    ("Auto",  0),
    ("1 min", 60),
    ("5 min", 300),
    ("15 min", 900),
    ("1 hr",  3_600),
    ("6 hr",  21_600),
    ("1 day", 86_400),
    ("1 wk",  604_800),
    ("1 mo",  2_592_000),
    ("1 yr",  31_536_000),
];

pub fn render_histogram(ui: &mut Ui, tab: &mut TabState) {
    // ── State machine for loading states ──────────────────────────────────
    if tab.histogram_data.is_none() {
        if tab.histogram_handle.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Analyzing timestamps…");
            });
        } else if matches!(tab.status, crate::tab::TabStatus::Ready) {
            ui.centered_and_justified(|ui| {
                ui.label("No timestamps detected in this file");
            });
        } else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Waiting for indexing…");
            });
        }
        return;
    }

    let data = tab.histogram_data.as_ref().unwrap();

    if data.buckets.is_empty() {
        ui.label("No timestamp data");
        return;
    }

    // ── Granularity selector ───────────────────────────────────────────────
    let current_secs = tab.histogram_granularity.unwrap_or(0); // 0 = auto
    let current_label = GRANULARITIES
        .iter()
        .find(|&&(_, s)| s == current_secs)
        .map(|&(l, _)| l)
        .unwrap_or("Auto");

    let mut new_secs: Option<i64> = None;
    ui.horizontal(|ui| {
        ui.label("Bin:");
        egui::ComboBox::from_id_salt("hist_granularity")
            .selected_text(current_label)
            .width(70.0)
            .show_ui(ui, |ui| {
                for &(label, secs) in GRANULARITIES {
                    if ui.selectable_value(&mut 0i64, secs, label).clicked() {
                        new_secs = Some(secs);
                    }
                }
            });
    });

    if let Some(secs) = new_secs {
        tab.rebin_histogram(if secs == 0 { None } else { Some(secs) });
        return; // data reference is now stale; render next frame
    }

    let data = tab.histogram_data.as_ref().unwrap();

    // ── Allocate drawing area ─────────────────────────────────────────────
    let available = ui.available_rect_before_wrap();
    let (rect, response) = ui.allocate_exact_size(available.size(), Sense::hover());

    if !ui.is_rect_visible(rect) {
        return;
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

    let padding = 4.0;
    let label_h = 18.0;
    let chart = Rect::from_min_max(
        rect.min + Vec2::new(padding, padding),
        rect.max - Vec2::new(padding, label_h),
    );

    if chart.width() < 4.0 || chart.height() < 4.0 {
        return;
    }

    let n = data.buckets.len();
    let max_count = data.max_count.max(1);
    let bar_w = (chart.width() / n as f32).max(1.0);
    let match_counts = data.compute_match_counts(&tab.search.matching_lines);

    // ── Draw bars ─────────────────────────────────────────────────────────
    for (i, &count) in data.buckets.iter().enumerate() {
        if count == 0 {
            continue;
        }

        let x = chart.left() + i as f32 * bar_w;
        let bar_h = (count as f32 / max_count as f32) * chart.height();
        let bar_rect = Rect::from_min_max(
            Pos2::new(x, chart.bottom() - bar_h),
            Pos2::new((x + bar_w - 0.5).min(chart.right()), chart.bottom()),
        );

        // Total count — steel blue
        painter.rect_filled(bar_rect, 0.0, Color32::from_rgb(70, 130, 180));

        // Match overlay — yellow
        let mc = match_counts.get(i).copied().unwrap_or(0);
        if mc > 0 {
            let match_h = (mc as f32 / max_count as f32) * chart.height();
            let match_rect = Rect::from_min_max(
                Pos2::new(x, chart.bottom() - match_h),
                bar_rect.max,
            );
            painter.rect_filled(match_rect, 0.0, Color32::from_rgb(255, 220, 50));
        }
    }

    // ── X axis line ───────────────────────────────────────────────────────
    painter.hline(
        chart.x_range(),
        chart.bottom(),
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
    );

    // ── X axis labels (sparse) ────────────────────────────────────────────
    let label_interval = (n / 8).max(1);
    for i in (0..n).step_by(label_interval) {
        let x = chart.left() + i as f32 * bar_w + bar_w / 2.0;
        let ts = data.bucket_ts(i);
        let label = format_bucket_label(ts, data.bucket_secs);
        painter.text(
            Pos2::new(x, rect.bottom() - 2.0),
            egui::Align2::CENTER_BOTTOM,
            label,
            FontId::proportional(10.0),
            ui.visuals().weak_text_color(),
        );
    }

    // ── Hover tooltip ─────────────────────────────────────────────────────
    if let Some(pos) = response.hover_pos() {
        let i = ((pos.x - chart.left()) / bar_w) as usize;
        if i < n && chart.contains(pos) {
            let count = data.buckets[i];
            let mc = match_counts.get(i).copied().unwrap_or(0);
            let ts = data.bucket_ts(i);
            let label = format_bucket_label(ts, data.bucket_secs);

            // Highlight the hovered bar
            let x = chart.left() + i as f32 * bar_w;
            let hover_rect = Rect::from_min_max(
                Pos2::new(x, chart.top()),
                Pos2::new((x + bar_w).min(chart.right()), chart.bottom()),
            );
            painter.rect_filled(
                hover_rect,
                0.0,
                Color32::from_rgba_premultiplied(255, 255, 255, 25),
            );

            response.on_hover_ui(|ui| {
                ui.label(format!("{label}"));
                ui.label(format!("{count} lines"));
                if mc > 0 {
                    ui.label(format!("{mc} matches"));
                }
            });
        }
    }
}
