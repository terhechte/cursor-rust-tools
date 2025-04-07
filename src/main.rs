mod cargo_remote;
mod context;
mod docs;
mod lsp;
mod mcp;
mod project;
mod ui;

use std::env::args;

use crate::ui::App;
use anyhow::Result;
use context::Context as ContextType;
use mcp::run_server;
use tracing::{error, info};
use tracing_subscriber::{
    EnvFilter, Layer, fmt::format::PrettyFields, layer::SubscriberExt, util::SubscriberInitExt,
};
use ui::apply_theme;

#[tokio::main]
async fn main() -> Result<()> {
    let log_layer = tracing_subscriber::fmt::layer()
        .event_format(tracing_subscriber::fmt::format().compact())
        .fmt_fields(PrettyFields::new())
        .boxed();

    tracing_subscriber::registry()
        .with(
            (EnvFilter::builder().try_from_env())
                .unwrap_or(EnvFilter::new("cursor_rust_tools=info")),
        )
        .with(log_layer)
        .init();

    let no_ui = args().any(|arg| arg == "--no-ui");

    let (sender, receiver) = flume::unbounded();
    let context = ContextType::new(4000, sender).await;
    context.load_config().await?;

    let final_context = context.clone();

    // Run the MCP Server
    let cloned_context = context.clone();
    tokio::spawn(async move {
        run_server(cloned_context).await.unwrap();
    });

    if no_ui {
        info!(
            "Running in CLI mode on port {}:{}",
            context.address_information().0,
            context.address_information().1
        );
        info!("Configuration file: {}", context.configuration_file());
        if context.project_descriptions().await.is_empty() {
            error!("No projects found, please run without `--no-ui` or edit configuration file");
            return Ok(());
        }
        info!(
            "Cursor mcp json (project/.cursor.mcp.json):\n```json\n{}\n```",
            context.mcp_configuration()
        );
        loop {
            while let Ok(notification) = receiver.try_recv() {
                info!("  {}", notification.description());
            }
        }
    } else {
        run_ui(context, receiver)?;
    }

    final_context.shutdown_all().await;

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
