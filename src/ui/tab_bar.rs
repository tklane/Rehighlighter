use egui::{Color32, Frame, RichText, Ui};

use crate::tab::{TabState, TabStatus};

/// Returns Some(index) if a tab was requested to close.
pub fn render_tab_bar(ui: &mut Ui, tabs: &mut Vec<TabState>, active: &mut usize) -> Option<usize> {
    let mut close_request: Option<usize> = None;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;

        for (i, tab) in tabs.iter().enumerate() {
            let is_active = i == *active;

            let bg = if is_active {
                ui.visuals().window_fill
            } else {
                ui.visuals().panel_fill
            };

            let frame = Frame::none()
                .fill(bg)
                .inner_margin(egui::Margin::symmetric(8.0, 4.0));

            let response = frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Spinner or status indicator
                    match &tab.status {
                        TabStatus::Indexing { .. } => {
                            ui.spinner();
                        }
                        TabStatus::Error(_) => {
                            ui.label(RichText::new("⚠").color(Color32::RED));
                        }
                        TabStatus::Ready => {}
                    }

                    let label = egui::Label::new(
                        RichText::new(&tab.title)
                            .strong_if(is_active)
                    )
                    .truncate();

                    ui.add(label);

                    // Close button
                    let close = ui.small_button(RichText::new("×").size(14.0));
                    if close.clicked() {
                        close_request = Some(i);
                    }
                });
            });

            // Click tab to switch
            if response.response.clicked() && !is_active {
                *active = i;
            }

            // Separator between tabs
            ui.separator();
        }

        // "+" button to open a new file
        if ui.button("+").clicked() {
            // Handled by caller via return value (None means no-op, caller checks)
        }
    });

    close_request
}

trait RichTextExt {
    fn strong_if(self, cond: bool) -> Self;
}

impl RichTextExt for RichText {
    fn strong_if(self, cond: bool) -> Self {
        if cond { self.strong() } else { self }
    }
}
