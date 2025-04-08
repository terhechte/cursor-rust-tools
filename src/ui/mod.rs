mod app;
mod log;
mod theme;

use std::sync::Arc;

use anyhow::Result;
use app::App;
use flume::Receiver;
use theme::apply_theme;

use crate::context::Context;
use crate::context::ContextNotification;

pub use app::ProjectDescription;

pub fn run_ui(
    context: Context,
    receiver: Receiver<ContextNotification>,
    project_descriptions: Vec<ProjectDescription>,
) -> Result<()> {
    let d = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/dock_icon.png"))
        .expect("The icon data must be valid");
    // Configure eframe options (window title, size, etc.)
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 800.0]) // Default window size
            .with_min_inner_size([400.0, 300.0]), // Minimum window size
        ..Default::default()
    };
    options.viewport.icon = Some(Arc::new(d));

    let app = App::new(context, receiver, project_descriptions);

    eframe::run_native(
        "Cursor Rust Tools",
        options,
        Box::new(|cc| {
            apply_theme(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Failed to run eframe: {}", e))
}
