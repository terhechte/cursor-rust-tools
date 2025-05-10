use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

use super::change_notifier::ChangeNotifier;
use super::client_state::ClientState;
use crate::lsp::{LspNotification, IndexingProgress};
use crate::project::Project;
use flume::Sender;

#[derive(Debug)]
pub struct RustAnalyzerLsp {
    project: Project,
    server: Arc<Mutex<ServerSocket>>,
    #[allow(dead_code)] // Keep the handle to ensure the mainloop runs
    mainloop_handle: Mutex<Option<JoinHandle<()>>>,
    indexed_rx: Mutex<flume::Receiver<()>>,
    #[allow(dead_code)] // Keep the handle to ensure the change notifier runs
    change_notifier: ChangeNotifier,
    // Track whether initial indexing is complete to avoid infinite reindexing
    initial_indexing_complete: AtomicBool,
}

impl RustAnalyzerLsp {
    pub async fn new(project: &Project, notifier: Sender<LspNotification>) -> Result<Self> {
        let (indexed_tx, indexed_rx) = flume::unbounded();
        
        // Create a clone early for use in the client state
        let notifier_for_client = notifier.clone();
        
        let (mainloop, server) = async_lsp::MainLoop::new_client(|_server| {
            ServiceBuilder::new()
                .layer(TracingLayer::default())
                .layer(LifecycleLayer::default()) // Handle init/shutdown automatically
                .layer(CatchUnwindLayer::default())
                .layer(ConcurrencyLayer::default())
                .service(ClientState::new_router(
                    indexed_tx,
                    notifier_for_client,
                    project.root().clone(),
                ))
        });

        // Check if rust-analyzer is available AND works correctly
        let is_installed = match tokio::process::Command::new("rust-analyzer")
            .arg("--version")  // Try to run with --version to check if it really works
            .output()
            .await {
                Ok(output) if output.status.success() => true,
                _ => false,  // Command exists but fails or doesn't exist at all
            };

        if !is_installed {
            // Attempt to install rust-analyzer using rustup if available
            tracing::warn!("rust-analyzer not found or not working properly. Attempting to install...");
            
            let rustup_check = tokio::process::Command::new("rustup")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
                
            let rustup_available = rustup_check.is_ok();
                
            if rustup_available {
                tracing::info!("Installing rust-analyzer with rustup...");
                
                // First try to detect the current toolchain
                let current_toolchain = match tokio::process::Command::new("rustup")
                    .args(["show", "active-toolchain"])
                    .output()
                    .await {
                        Ok(output) if output.status.success() => {
                            let toolchain = String::from_utf8_lossy(&output.stdout)
                                .split_whitespace()
                                .next()
                                .map(|s| s.to_string());
                            tracing::info!("Detected current toolchain: {:?}", toolchain);
                            toolchain
                        },
                        _ => None,
                    };
                
                // Command with toolchain if detected
                let install_result = if let Some(toolchain) = current_toolchain {
                    tokio::process::Command::new("rustup")
                        .args(["component", "add", "rust-analyzer", "--toolchain", &toolchain])
                        .output()
                        .await
                } else {
                    // Fallback to generic command
                    tokio::process::Command::new("rustup")
                        .args(["component", "add", "rust-analyzer"])
                        .output()
                        .await
                };
                
                match install_result {
                    Ok(output) if output.status.success() => {
                        tracing::info!("Successfully installed rust-analyzer");
                        
                        // Verify installation was successful by checking if command works now
                        let verify_success = match tokio::process::Command::new("rust-analyzer")
                            .arg("--version") 
                            .output()
                            .await {
                                Ok(output) if output.status.success() => true,
                                _ => false,
                            };
                            
                        if !verify_success {
                            // On Windows, we might need to check for rust-analyzer.exe
                            if cfg!(windows) {
                                tracing::warn!("rust-analyzer command still not working, checking rust-analyzer.exe path");
                                
                                // Try to locate the rust-analyzer binary in the user's .cargo/bin directory
                                let home_dir = dirs::home_dir();
                                if let Some(home) = home_dir {
                                    let cargo_bin = home.join(".cargo").join("bin").join("rust-analyzer.exe");
                                    
                                    if cargo_bin.exists() {
                                        tracing::info!("Found rust-analyzer at: {:?}", cargo_bin);
                                    } else {
                                        tracing::warn!("rust-analyzer.exe not found at expected path: {:?}", cargo_bin);
                                    }
                                }
                                
                                // Provide clearer error messages for Windows users
                                tracing::info!("On Windows, you might need to run: rustup component add rust-analyzer --toolchain stable-x86_64-pc-windows-msvc");
                            }
                        }
                    },
                    Ok(output) => {
                        let error = String::from_utf8_lossy(&output.stderr);
                        tracing::error!("Failed to install rust-analyzer with rustup: {}", error);
                        // Handle the common "Unknown binary" error on Windows
                        if error.contains("Unknown binary") && error.contains("rust-analyzer") {
                            tracing::info!("Specific Windows solution: Try running 'rustup component add rust-analyzer --toolchain stable-x86_64-pc-windows-msvc'");
                        }
                        return Err(anyhow::anyhow!(
                            "Failed to install rust-analyzer automatically: {}. Please install it manually with 'rustup component add rust-analyzer'", 
                            error
                        ));
                    },
                    Err(e) => {
                        tracing::error!("Error running rustup: {}", e);
                        return Err(anyhow::anyhow!(
                            "Failed to run rustup to install rust-analyzer: {}. Please install it manually.", e
                        ));
                    }
                }
            } else {
                return Err(anyhow::anyhow!(
                    "rust-analyzer not found. Please install rustup and run 'rustup component add rust-analyzer', or install rust-analyzer manually."
                ));
            }
        }

        let process = match async_process::Command::new("rust-analyzer")
            .current_dir(project.root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn() {
                Ok(process) => process,
                Err(e) => {
                    // First attempt failed, try to locate rust-analyzer in standard paths
                    tracing::warn!("Failed to run rust-analyzer directly: {}", e);
                    
                    if cfg!(windows) {
                        // Try to locate rust-analyzer in standard Windows locations
                        let mut rust_analyzer_path = None;
                        
                        // Check in .cargo/bin
                        if let Some(home_dir) = dirs::home_dir() {
                            let cargo_bin_path = home_dir.join(".cargo").join("bin").join("rust-analyzer.exe");
                            if cargo_bin_path.exists() {
                                tracing::info!("Found rust-analyzer.exe at: {:?}", cargo_bin_path);
                                rust_analyzer_path = Some(cargo_bin_path);
                            }
                        }
                        
                        if let Some(path) = rust_analyzer_path {
                            match async_process::Command::new(path)
                                .current_dir(project.root())
                                .stdin(Stdio::piped())
                                .stdout(Stdio::piped())
                                .stderr(Stdio::inherit())
                                .spawn() {
                                    Ok(process) => process,
                                    Err(e) => {
                                        return Err(anyhow::anyhow!(
                                            "Failed to run rust-analyzer from found path: {}. Try manually installing with 'rustup component add rust-analyzer --toolchain stable-x86_64-pc-windows-msvc'", e
                                        ));
                                    }
                                }
                        } else {
                            return Err(anyhow::anyhow!(
                                "Could not locate rust-analyzer executable. Please run 'rustup component add rust-analyzer --toolchain stable-x86_64-pc-windows-msvc' to install it."
                            ));
                        }
                    } else {
                        // For non-Windows platforms, just report the original error
                        return Err(anyhow::anyhow!(
                            "Failed to run rust-analyzer: {}. Please make sure rust-analyzer is installed and available in your PATH.", e
                        ));
                    }
                }
            };

        let stdout = process.stdout.context("Failed to get stdout")?;
        let stdin = process.stdin.context("Failed to get stdin")?;

        let mainloop_handle = tokio::spawn(async move {
            match mainloop.run_buffered(stdout, stdin).await {
                Ok(()) => debug!("LSP mainloop finished gracefully."),
                Err(e) => tracing::error!("LSP mainloop finished with error: {}", e),
            }
        });

        let server = Arc::new(Mutex::new(server));

        // Get the current runtime handle
        let handle = tokio::runtime::Handle::current();
        let change_notifier = ChangeNotifier::new(server.clone(), project, handle)?;

        let client = Self {
            project: project.clone(),
            server,
            mainloop_handle: Mutex::new(Some(mainloop_handle)),
            indexed_rx: Mutex::new(indexed_rx),
            change_notifier,
            initial_indexing_complete: AtomicBool::new(false),
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
            .await?;
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
        
        // Start a background task to handle initial indexing completion
        let project_path = project.root().clone();
        let notifier_clone2 = notifier.clone();
        client.initial_indexing_complete.store(false, Ordering::SeqCst);
        let _task = tokio::spawn(async move {
            // Create progress instance for this thread
            let mut progress = IndexingProgress::new(project_path.clone());
            progress.start_indexing();
            
            // We only care about the first completion signal
            if let Ok(()) = rx.recv_async().await {
                info!("rust-analyzer initial indexing finished");
                
                // Mark indexing as complete
                progress.complete_indexing();
                
                // Send explicit "indexing finished" notification to update UI
                if let Err(e) = notifier_clone2.try_send(LspNotification::IndexingProgress(progress)) {
                    if matches!(e, flume::TrySendError::Disconnected(_)) {
                        tracing::debug!("Channel closed when sending indexing completion: {}", e);
                    } else {
                        tracing::error!("Failed to send indexing completion: {}", e);
                    }
                }
                
                // Also send the legacy notification for backward compatibility
                if let Err(e) = notifier_clone2.try_send(LspNotification::Indexing {
                    project: project_path,
                    is_indexing: false,
                }) {
                    if matches!(e, flume::TrySendError::Disconnected(_)) {
                        tracing::debug!("Channel closed when sending legacy indexing status: {}", e);
                    } else {
                        tracing::error!("Failed to send legacy indexing status: {}", e);
                    }
                }
                
                // Drain any additional signals without processing them
                let mut drain_count = 0;
                while let Ok(()) = rx.try_recv() {
                    drain_count += 1;
                }
                if drain_count > 0 {
                    tracing::debug!("Drained {} additional indexing signals", drain_count);
                }
            }
        });

        Ok(client)
    }

    pub async fn shutdown(&self) -> Result<()> {
        // Try to acquire the lock with a timeout to avoid deadlock
        let server_lock_result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            self.server.lock()
        ).await;

        // Handle timeout or lock acquisition errors
        let mut server_guard = match server_lock_result {
            Ok(guard) => guard,
            Err(_) => {
                tracing::warn!("Timeout acquiring server lock during shutdown");
                // Return success since this isn't fatal
                return Ok(());
            }
        };

        // Try shutdown but don't fail if it errors
        if let Err(e) = server_guard.shutdown(()).await {
            tracing::warn!("Error during LSP shutdown request: {:?}", e);
            // Continue with exit anyway
        }

        // Try exit but don't fail if it errors
        if let Err(e) = server_guard.exit(()) {
            tracing::warn!("Error during LSP exit notification: {:?}", e);
        }

        // Release server lock before waiting for mainloop
        drop(server_guard);

        // Try to get the mainloop handle with a timeout
        let mainloop_lock_result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            self.mainloop_handle.lock()
        ).await;

        // Handle timeout or lock acquisition errors for mainloop handle
        match mainloop_lock_result {
            Ok(mut guard) => {
                // Only join if there's a handle to take
                if let Some(handle) = guard.take() {
                    // Don't wait indefinitely - use a timeout
                    match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
                        Ok(join_result) => {
                            if let Err(e) = join_result {
                                tracing::warn!("Error joining LSP mainloop task: {:?}", e);
                            }
                        },
                        Err(_) => tracing::warn!("Timeout waiting for LSP mainloop to finish"),
                    }
                }
            },
            Err(_) => tracing::warn!("Timeout acquiring mainloop handle lock during shutdown"),
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn open_file(&self, relative_path: impl AsRef<Path>, text: String) -> Result<()> {
        let path_ref = relative_path.as_ref();
        let uri = self.project.file_uri(path_ref)?;
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

        // Check if indexing is already complete
        if self.initial_indexing_complete.load(Ordering::SeqCst) {
            // Skip waiting for indexing signals if we've already completed indexing
            tracing::debug!("Skipping indexing wait for file (already indexed)");
            return Ok(());
        }

        tracing::debug!("Waiting for indexing to complete for file: {:?}", path_ref);
        
        // Wait for indexing to complete
        self.indexed_rx
            .lock()
            .await
            .recv_async()
            .await
            .context("Failed waiting for index")?;
        
        // Mark indexing as complete
        self.initial_indexing_complete.store(true, Ordering::SeqCst);
        tracing::debug!("Indexing completed while opening file: {:?}", path_ref);
        
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
