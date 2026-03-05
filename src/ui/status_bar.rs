use egui::Ui;

use crate::tab::{TabState, TabStatus};

pub fn render_status_bar(ui: &mut Ui, tab: &TabState) {
    ui.horizontal(|ui| {
        // File path
        ui.label(tab.path.to_string_lossy().as_ref());

        ui.separator();

        // File size
        let size = tab.mmap.len();
        ui.label(format_size(size));

        ui.separator();

        match &tab.status {
            TabStatus::Indexing { progress_pct } => {
                ui.spinner();
                ui.label(format!("Indexing {:.0}%", progress_pct));
            }
            TabStatus::Ready => {
                ui.label(format!("{} lines", tab.index.line_count()));
            }
            TabStatus::Error(e) => {
                ui.label(format!("Error: {e}"));
            }
        }

        // Match count
        if !tab.search.matching_lines.is_empty() {
            ui.separator();
            let count = tab.search.matching_lines.len();
            ui.label(format!("{count} match{}", if count == 1 { "" } else { "es" }));
        }
    });
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}
