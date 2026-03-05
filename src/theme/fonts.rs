use egui::{FontData, FontDefinitions, FontFamily};

#[cfg(target_os = "macos")]
const UI_FONT: &str = "SF Pro Text";
#[cfg(target_os = "macos")]
const MONO_FONT: &str = "SF Mono";

#[cfg(target_os = "windows")]
const UI_FONT: &str = "Segoe UI";
#[cfg(target_os = "windows")]
const MONO_FONT: &str = "Cascadia Code";

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const UI_FONT: &str = "DejaVu Sans";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const MONO_FONT: &str = "DejaVu Sans Mono";

/// Load system fonts and inject them into egui's FontDefinitions.
/// Falls back gracefully to egui's built-in fonts if system fonts cannot be loaded.
pub fn load_system_fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    if let Some(data) = load_font_by_family(UI_FONT) {
        fonts
            .font_data
            .insert("system_ui".to_owned(), FontData::from_owned(data));
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "system_ui".to_owned());
    }

    if let Some(data) = load_font_by_family(MONO_FONT) {
        fonts
            .font_data
            .insert("system_mono".to_owned(), FontData::from_owned(data));
        fonts
            .families
            .get_mut(&FontFamily::Monospace)
            .unwrap()
            .insert(0, "system_mono".to_owned());
    }

    fonts
}

fn load_font_by_family(family_name: &str) -> Option<Vec<u8>> {
    use font_kit::{
        family_name::FamilyName,
        handle::Handle,
        properties::Properties,
        source::SystemSource,
    };

    let source = SystemSource::new();
    let handle = source
        .select_best_match(
            &[FamilyName::Title(family_name.to_owned())],
            &Properties::new(),
        )
        .ok()?;

    match handle {
        Handle::Path { path, .. } => std::fs::read(&path).ok(),
        Handle::Memory { bytes, .. } => Some(bytes.to_vec()),
    }
}
