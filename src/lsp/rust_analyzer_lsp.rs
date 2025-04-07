use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result};
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::server::LifecycleLayer;
use async_lsp::tracing::TracingLayer;
use async_lsp::{LanguageServer, ServerSocket};
use lsp_types::request::GotoTypeDefinitionParams;
use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentParams, DocumentSymbolClientCapabilities,
    GotoDefinitionResponse, Hover, HoverClientCapabilities, HoverParams, InitializeParams,
    InitializedParams, Location, MarkupKind, Position, ReferenceContext, ReferenceParams,
    TextDocumentClientCapabilities, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, WindowClientCapabilities, WorkDoneProgressParams, WorkspaceFolder,
};
use serde_json::json;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tower::ServiceBuilder;
use tracing::{debug, info};

use super::client_state::ClientState;
use crate::lsp::LspNotification;
use crate::project::Project;
use flume::Sender;

#[derive(Debug)]
pub struct RustAnalyzerLsp {
    project: Project,
    server: Mutex<ServerSocket>,
    #[allow(dead_code)] // Keep the handle to ensure the mainloop runs
    mainloop_handle: Mutex<Option<JoinHandle<()>>>,
    indexed_rx: Mutex<flume::Receiver<()>>,
}

impl RustAnalyzerLsp {
    pub async fn new(project: &Project, notifier: Sender<LspNotification>) -> Result<Self> {
        let (indexed_tx, indexed_rx) = flume::unbounded();
        let (mainloop, server) = async_lsp::MainLoop::new_client(|_server| {
            ServiceBuilder::new()
                .layer(TracingLayer::default())
                .layer(LifecycleLayer::default()) // Handle init/shutdown automatically
                .layer(CatchUnwindLayer::default())
                .layer(ConcurrencyLayer::default())
                .service(ClientState::new_router(
                    indexed_tx,
                    notifier,
                    project.root().to_path_buf(),
                ))
        });

        let process = async_process::Command::new("rust-analyzer")
            .current_dir(project.root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed run rust-analyzer")?;

        let stdout = process.stdout.context("Failed to get stdout")?;
        let stdin = process.stdin.context("Failed to get stdin")?;

        let mainloop_handle = tokio::spawn(async move {
            match mainloop.run_buffered(stdout, stdin).await {
                Ok(()) => debug!("LSP mainloop finished gracefully."),
                Err(e) => tracing::error!("LSP mainloop finished with error: {}", e),
            }
        });

        let client = Self {
            project: project.clone(),
            server: Mutex::new(server),
            mainloop_handle: Mutex::new(Some(mainloop_handle)),
            indexed_rx: Mutex::new(indexed_rx),
        };

        // Initialize.
        let init_ret = client
            .server
            .lock()
            .await
            .initialize(InitializeParams {
                workspace_folders: Some(vec![WorkspaceFolder {
                    uri: project.uri()?,
                    name: "root".into(),
                }]),
                capabilities: ClientCapabilities {
                    window: Some(WindowClientCapabilities {
                        work_done_progress: Some(true), // Required for indexing progress
                        ..WindowClientCapabilities::default()
                    }),
                    text_document: Some(TextDocumentClientCapabilities {
                        document_symbol: Some(DocumentSymbolClientCapabilities {
                            // Flat symbols are easier to process for us
                            hierarchical_document_symbol_support: Some(false),
                            ..DocumentSymbolClientCapabilities::default()
                        }),
                        hover: Some(HoverClientCapabilities {
                            content_format: Some(vec![MarkupKind::Markdown]),
                            ..HoverClientCapabilities::default()
                        }),
                        ..TextDocumentClientCapabilities::default()
                    }),
                    experimental: Some(json!({
                        "hoverActions": true
                    })),
                    ..ClientCapabilities::default()
                },
                ..InitializeParams::default()
            })
            .await
            .context("LSP initialize failed")?;
        tracing::trace!("Initialized: {init_ret:?}");
        info!("LSP Initialized");

        client
            .server
            .lock()
            .await
            .initialized(InitializedParams {})
            .context("Sending Initialized notification failed")?;

        info!("Waiting for rust-analyzer indexing...");
        let rx = client.indexed_rx.lock().await.clone();
        tokio::spawn(async move {
            while let Ok(()) = rx.recv_async().await {
                info!("rust-analyzer indexing finished.");
            }
        });

        Ok(client)
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.server
            .lock()
            .await
            .shutdown(())
            .await
            .context("Sending Shutdown request failed")?;
        self.server
            .lock()
            .await
            .exit(())
            .context("Sending Exit notification failed")?;

        // Wait for the mainloop to finish. This implicitly waits for the process to exit.
        if let Err(e) = self.mainloop_handle.lock().await.take().unwrap().await {
            tracing::error!("Error joining LSP mainloop task: {:?}", e);
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn open_file(&self, relative_path: impl AsRef<Path>, text: String) -> Result<()> {
        let uri = self.project.file_uri(relative_path)?;
        self.server
            .lock()
            .await
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "rust".into(), // Assuming Rust, could be made generic
                    version: 0,                 // Start with version 0
                    text,
                },
            })
            .context("Sending DidOpen notification failed")?;
        self.indexed_rx
            .lock()
            .await
            .recv_async()
            .await
            .context("Failed waiting for index")?;
        Ok(())
    }

    pub async fn hover(
        &self,
        relative_path: impl AsRef<Path>,
        position: Position,
    ) -> Result<Option<Hover>> {
        let uri = self.project.file_uri(relative_path)?;
        self.server
            .lock()
            .await
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .context("Hover request failed")
    }

    pub async fn type_definition(
        &self,
        relative_path: impl AsRef<Path>,
        position: Position,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = self.project.file_uri(relative_path)?;
        self.server
            .lock()
            .await
            .type_definition(GotoTypeDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: Default::default(),
            })
            .await
            .context("Type definition request failed")
    }

    pub async fn find_references(
        &self,
        relative_path: impl AsRef<Path>,
        position: Position,
    ) -> Result<Option<Vec<Location>>> {
        let uri = self.project.file_uri(relative_path)?;
        self.server
            .lock()
            .await
            .references(ReferenceParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            })
            .await
            .context("References request failed")
    }

    pub async fn document_symbols(
        &self,
        relative_path: impl AsRef<Path>,
    ) -> Result<Option<Vec<lsp_types::SymbolInformation>>> {
        let uri = self.project.file_uri(relative_path)?;
        let o = self
            .server
            .lock()
            .await
            .document_symbol(lsp_types::DocumentSymbolParams {
                text_document: TextDocumentIdentifier { uri },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: Default::default(),
            })
            .await
            .context("Document symbols request failed")?
            .and_then(|symbols| match symbols {
                lsp_types::DocumentSymbolResponse::Flat(f) => Some(f),
                lsp_types::DocumentSymbolResponse::Nested(_) => {
                    tracing::error!("Only support flat symbols for now");
                    None
                }
            });
        Ok(o)
    }
}
