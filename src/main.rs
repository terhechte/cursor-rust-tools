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
use tokio::signal;
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
    let server_handle = tokio::spawn(async move {
        run_server(cloned_context).await.unwrap();
    });

    let main_loop_fut = async {
        if no_ui {
            info!(
                "Running in CLI mode on port {}:{}",
                context.address_information().0,
                context.address_information().1
            );
            info!("Configuration file: {}", context.configuration_file());
            if context.project_descriptions().await.is_empty() {
                error!(
                    "No projects found, please run without `--no-ui` or edit configuration file"
                );
                return Ok(()); // Early return for no projects in CLI mode
            }
            info!(
                "Cursor mcp json (project/.cursor.mcp.json):\n```json\n{}\n```",
                context.mcp_configuration()
            );
            // Keep the CLI mode running indefinitely until Ctrl+C
            loop {
                while let Ok(notification) = receiver.try_recv() {
                    info!("  {}", notification.description());
                }
                // Add a small sleep to avoid busy-waiting if desired, or just rely on Ctrl+C
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
            // Note: This loop will now only exit via Ctrl+C handled by tokio::select!
        } else {
            let project_descriptions = context.project_descriptions().await;
            // run_ui blocks, so we need to handle its potential error
            run_ui(context, receiver, project_descriptions)
        }
    };

    tokio::select! {
        res = main_loop_fut => {
            if let Err(e) = res {
                error!("Main loop finished with error: {}", e);
            } else {
                info!("Main loop finished normally.");
            }
        },
        _ = signal::ctrl_c() => {
            info!("Ctrl+C received, shutting down...");
        }
        _ = server_handle => {
             info!("Server task finished unexpectedly.");
        }
    }

    if no_ui {
        final_context.shutdown_all().await;
    }

    Ok(())
}

fn run_ui(
    context: context::Context,
    receiver: flume::Receiver<context::ContextNotification>,
    project_descriptions: Vec<ui::ProjectDescription>,
) -> Result<()> {
    // Configure eframe options (window title, size, etc.)
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 800.0]) // Default window size
            .with_min_inner_size([400.0, 300.0]), // Minimum window size
        ..Default::default()
    };

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
