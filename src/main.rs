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

    let project = Project::new(
        "/Users/terhechte/Developer/Rust/supatest",
        project::TransportType::Sse {
            host: "127.0.0.1".to_string(),
            port: 4000,
        },
    )
    .context("Failed to create project")?;

    let context = ContextType::new(project.clone()).await?;
    // mcp::run_server(&context)
    //     .await
    //     .context("Failed to run MCP server")?;

    let mut lsp = RustAnalyzerLsp::new(&project)
        .await
        .context("Failed to initialize RustAnalyzerLsp")?;

    let symbols = lsp
        .document_symbols(&project, "src/main.rs")
        .await
        .context("Failed to get document symbols")?;
    info!("Document symbols: {symbols:#?}");

    // // Synchronize documents.
    // let file_path = "src/main.rs";
    // let text = "#![no_std] fn func() { let var = 1; }".to_string();
    // lsp.open_file(&project, file_path, text.clone())
    //     .await
    //     .context("Failed to open file")?;

    // // Query.
    let text = "ggg".to_string();
    let var_pos = text.find("var").unwrap();
    let ooox: String = "api_key".to_string();
    let hover = lsp
        // `HeaderMap`
        .hover(&project, "src/main.rs", Position::new(130, 18))
        .await
        .context("Hover request failed")?
        .context("Hover request returned None")?; // Expecting some result
    dbg!(&hover);

    // info!("Hover result: {hover:?}");
    // assert!(
    //     matches!(
    //         hover.contents,
    //         HoverContents::Markup(MarkupContent { value, .. })
    //         if value.contains("let var: i32")
    //     ),
    //     "should show the type of `var`",
    // );

    let type_definition = lsp
        .type_definition(&project, "src/main.rs", Position::new(130, 18))
        .await
        .context("Failed to get type definition")?;
    info!("Type definition: {type_definition:#?}");

    let references = lsp
        .find_references(&project, "src/main.rs", Position::new(130, 18))
        .await
        .context("Failed to get references")?;
    info!("References: {references:#?}");

    // tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    // // Shutdown.
    lsp.shutdown().await.context("LSP shutdown failed")?;

    Ok(())
}
