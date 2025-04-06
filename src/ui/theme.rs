use std::{collections::BTreeMap, sync::Arc};

use egui::{FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use egui_aesthetix::Aesthetix;

pub fn apply_theme(ctx: &egui::Context) {
    let (fonts, text_styles) = font_definitions();
    ctx.set_fonts(fonts);
    ctx.set_style(Arc::new(egui_aesthetix::themes::NordDark.custom_style()));
    ctx.style_mut(|style| style.text_styles = text_styles);
}

fn font_definitions() -> (FontDefinitions, BTreeMap<TextStyle, FontId>) {
    let mut fonts = FontDefinitions::default();
    //Install my own font (maybe supporting non-latin characters):
    fonts.font_data.insert(
        "OpenSans".to_owned(),
        FontData::from_static(include_bytes!("../../assets/OpenSans-Regular.ttf")),
    );
    // Put my font first (highest priority):
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "OpenSans".to_owned());

    use FontFamily::{Monospace, Proportional};
    return (
        fonts,
        [
            (TextStyle::Small, FontId::new(10.0, Proportional)),
            (TextStyle::Body, FontId::new(12.0, Proportional)),
            (TextStyle::Monospace, FontId::new(12.0, Monospace)),
            (TextStyle::Button, FontId::new(12.0, Proportional)),
            (TextStyle::Heading, FontId::new(16.0, Proportional)),
        ]
        .into(),
    );
}
