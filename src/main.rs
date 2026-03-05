mod app;
mod indexer;
mod overview_cache;
mod search;
mod tab;
mod theme;
mod timestamp;
mod ui;

use app::AppState;
use theme::{apply_theme, fonts::load_system_fonts};

fn main() -> eframe::Result<()> {
    env_logger::init();

    // Allow opening a file from the command line
    let initial_file: Option<std::path::PathBuf> = std::env::args().nth(1).map(Into::into);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Rehighlighter")
            .with_min_inner_size([600.0, 400.0])
            .with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Rehighlighter",
        native_options,
        Box::new(move |cc| {
            // Load system fonts before the first frame
            cc.egui_ctx.set_fonts(load_system_fonts());

            // Apply system theme immediately
            let theme = theme::AppTheme::detect();
            apply_theme(&cc.egui_ctx, theme);

            let mut state = AppState::new();
            state.theme = theme;

            // Open file passed on the command line
            if let Some(path) = initial_file {
                state.open_file(path);
            }

            Ok(Box::new(state))
        }),
    )
}
