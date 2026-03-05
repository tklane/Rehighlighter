use egui::{FontId, ScrollArea, TextEdit, Ui};

use crate::tab::TabState;

pub fn render_detail_panel(ui: &mut Ui, tab: &mut TabState) {
    ui.horizontal(|ui| {
        ui.heading("Line Detail");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                tab.detail_open = false;
            }
        });
    });
    ui.separator();

    let Some(line_num) = tab.detail_line else {
        ui.label("No line selected. Click a log line to view it here.");
        return;
    };

    let content = match tab.index.line_byte_range(line_num) {
        Some(range) => tab.mmap.line_str(range).into_owned(),
        None => {
            ui.label("Line not yet indexed.");
            return;
        }
    };

    ui.label(format!("Line {}", line_num + 1));
    ui.separator();

    ScrollArea::vertical().id_salt("detail_scroll").show(ui, |ui| {
        let mut text = content.as_str();
        ui.add(
            TextEdit::multiline(&mut text)
                .desired_width(f32::INFINITY)
                .desired_rows(15)
                .font(FontId::monospace(12.0))
                .interactive(false),
        );
    });

    ui.separator();
    if ui.button("Copy to clipboard").clicked() {
        ui.output_mut(|o| o.copied_text = content.clone());
    }
}
