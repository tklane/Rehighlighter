pub mod fonts;

use egui::{Color32, Context, Visuals};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum AppTheme {
    #[default]
    Light,
    Dark,
}

impl AppTheme {
    pub fn detect() -> Self {
        match dark_light::detect() {
            dark_light::Mode::Dark => AppTheme::Dark,
            _ => AppTheme::Light,
        }
    }
}

pub fn apply_theme(ctx: &Context, theme: AppTheme) {
    match theme {
        AppTheme::Dark => {
            let mut v = Visuals::dark();
            // Platform-matched dark colors
            v.panel_fill = Color32::from_rgb(28, 28, 30);
            v.window_fill = Color32::from_rgb(44, 44, 46);
            v.extreme_bg_color = Color32::from_rgb(18, 18, 18);
            v.faint_bg_color = Color32::from_rgb(36, 36, 38);
            ctx.set_visuals(v);
        }
        AppTheme::Light => {
            let mut v = Visuals::light();
            v.panel_fill = Color32::from_rgb(246, 246, 246);
            v.window_fill = Color32::WHITE;
            v.extreme_bg_color = Color32::WHITE;
            v.faint_bg_color = Color32::from_rgb(240, 240, 245);
            ctx.set_visuals(v);
        }
    }
}
