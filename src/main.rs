mod context;
mod docs;
mod lsp;
mod mcp;
mod project;

use anyhow::{Context, Result};
use context::Context as ContextType;
use lsp::RustAnalyzerLsp;
use lsp_types::{HoverContents, MarkupContent, Position};
use project::Project;
use tracing::{Level, info};

#[tokio::main]
async fn main() -> Result<()> {
    // Keep tracing setup in main
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();

    let project = Project::new("/Users/terhechte/Developer/Rust/supatest")
        .context("Failed to create project")?;

    let (sender, receiver) = flume::unbounded();
    let context = ContextType::new(4000, sender);
    context.add_project(project).await?;

    Ok(())
}
