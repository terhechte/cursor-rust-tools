mod context;
mod docs;
mod lsp;
mod mcp;
mod project;
mod ui;

use crate::ui::App;
use anyhow::Result;
use context::Context as ContextType;
use mcp::run_server;
use tracing::Level;
use ui::apply_theme;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();

    let (sender, receiver) = flume::unbounded();
    let context = ContextType::new(4000, sender).await;
    context.load_config().await?;

    // Run the MCP Server
    let cloned_context = context.clone();
    tokio::spawn(async move {
        run_server(cloned_context).await.unwrap();
    });

    run_ui(context, receiver)?;

    Ok(())
}

fn run_ui(
    context: context::Context,
    receiver: flume::Receiver<context::ContextNotification>,
) -> Result<()> {
    // Configure eframe options (window title, size, etc.)
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 800.0]) // Default window size
            .with_min_inner_size([400.0, 300.0]), // Minimum window size
        ..Default::default()
    };

    let app = App::new(context, receiver);

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
