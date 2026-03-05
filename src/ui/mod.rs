pub mod detail_panel;
pub mod histogram;
pub mod log_view;
pub mod overview;
pub mod search_bar;
pub mod status_bar;
pub mod tab_bar;

/// Color palette for multi-term search highlighting.
/// Term i uses TERM_COLORS[i % TERM_COLORS.len()] as the background.
pub const TERM_COLORS: &[egui::Color32] = &[
    egui::Color32::from_rgb(255, 220,  50), // yellow
    egui::Color32::from_rgb( 80, 200, 230), // cyan
    egui::Color32::from_rgb(100, 220,  80), // lime
    egui::Color32::from_rgb(255, 150,  60), // orange
    egui::Color32::from_rgb(220, 100, 255), // purple
];
