use std::ops::ControlFlow;
use std::path::PathBuf;

use super::Stop;
use crate::lsp::{LspNotification, IndexingProgress};
use async_lsp::router::Router;
use async_lsp::{LanguageClient, ResponseError};
use lsp_types::{
    NumberOrString, ProgressParams, ProgressParamsValue, PublishDiagnosticsParams,
    ShowMessageParams, WorkDoneProgress,
};

// Old and new token names.
const RA_INDEXING_TOKENS: &[&str] = &[
    "rustAnalyzer/Indexing",
    "rustAnalyzer/cachePriming",
    "rustAnalyzer/Building",
];

pub struct ClientState {
    project: PathBuf,
    indexed_tx: Option<flume::Sender<()>>,
    notifier: flume::Sender<LspNotification>,
}

impl LanguageClient for ClientState {
    type Error = ResponseError;
    type NotifyResult = ControlFlow<async_lsp::Result<()>>;

    fn progress(&mut self, params: ProgressParams) -> Self::NotifyResult {
        tracing::trace!("{:?} {:?}", params.token, params.value);
        let is_indexing =
            matches!(params.token, NumberOrString::String(ref s) if RA_INDEXING_TOKENS.contains(&s.as_str()));
        let is_work_done = matches!(
            params.value,
            ProgressParamsValue::WorkDone(WorkDoneProgress::End(_))
        );
        
        // Extract more detailed progress information if available
        let progress_message = match &params.value {
            ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(begin)) => {
                tracing::debug!("Indexing Begin: token={:?}, title={:?}", params.token, begin.title);
                Some(begin.title.clone())
            },
            ProgressParamsValue::WorkDone(WorkDoneProgress::Report(report)) => {
                tracing::debug!("Indexing Report: token={:?}, message={:?}, percentage={:?}", 
                             params.token, report.message, report.percentage);
                report.message.clone()
            },
            ProgressParamsValue::WorkDone(WorkDoneProgress::End(end)) => {
                tracing::debug!("Indexing End: token={:?}, message={:?}", params.token, end.message);
                end.message.clone()
            },
            _ => None,
        };
        
        // Extract percentage if available
        let progress_percentage = match &params.value {
            ProgressParamsValue::WorkDone(WorkDoneProgress::Report(report)) => {
                report.percentage.map(|p| p as f32)
            },
            _ => None,
        };
        
        // Handle detailed indexing progress notifications
        if is_indexing {
            if is_work_done {
                tracing::debug!("Rust-analyzer indexing work done event");
                
                // Create a complete progress notification
                let mut progress = IndexingProgress::new(self.project.clone());
                progress.complete_indexing();
                
                // Try to send the detailed progress notification
                if let Err(e) = self.notifier.try_send(LspNotification::IndexingProgress(progress)) {
                    if matches!(e, flume::TrySendError::Disconnected(_)) {
                        tracing::debug!("Channel closed when sending progress completion: {}", e);
                    } else {
                        tracing::error!("Failed to send progress completion: {}", e);
                    }
                }
                
                // Also send the legacy indexing completion signal
                if let Err(e) = self.notifier.try_send(LspNotification::Indexing {
                    project: self.project.clone(),
                    is_indexing: false,
                }) {
                    if matches!(e, flume::TrySendError::Disconnected(_)) {
                        tracing::debug!("Channel closed when sending indexing end: {}", e);
                    } else {
                        tracing::error!("Failed to send indexing notification: {}", e);
                    }
                }

                // Send the completion signal
                if let Some(tx) = &self.indexed_tx {
                    if let Err(e) = tx.try_send(()) {
                        if matches!(e, flume::TrySendError::Disconnected(_)) {
                            tracing::debug!("Channel closed when sending indexing completion: {}", e);
                        } else {
                            tracing::error!("Failed to send indexing completion signal: {}", e);
                        }
                    }
                }
            } else {
                tracing::debug!("Rust-analyzer indexing work progress: {:?} {:?}", 
                              progress_message, progress_percentage);
                
                // Create an in-progress notification with details
                let mut progress = IndexingProgress::new(self.project.clone());
                progress.start_indexing();
                
                // Add detailed information if available
                if let Some(msg) = progress_message {
                    progress.status_message = Some(msg);
                }
                
                if let Some(percent) = progress_percentage {
                    progress.progress_percentage = Some(percent);
                }
                
                // Try to send the detailed progress notification
                if let Err(e) = self.notifier.try_send(LspNotification::IndexingProgress(progress)) {
                    if matches!(e, flume::TrySendError::Disconnected(_)) {
                        tracing::debug!("Channel closed when sending progress update: {}", e);
                    } else {
                        tracing::error!("Failed to send progress update: {}", e);
                    }
                }
                
                // Also send the legacy in-progress notification
                if let Err(e) = self.notifier.try_send(LspNotification::Indexing {
                    project: self.project.clone(),
                    is_indexing: true,
                }) {
                    if matches!(e, flume::TrySendError::Disconnected(_)) {
                        tracing::debug!("Channel closed when sending indexing start: {}", e);
                    } else {
                        tracing::error!("Failed to send indexing notification: {}", e);
                    }
                }
            }
        }
        
        ControlFlow::Continue(())
    }

    fn publish_diagnostics(&mut self, _: PublishDiagnosticsParams) -> Self::NotifyResult {
        ControlFlow::Continue(())
    }

    fn show_message(&mut self, params: ShowMessageParams) -> Self::NotifyResult {
        tracing::debug!("Message {:?}: {}", params.typ, params.message);
        ControlFlow::Continue(())
    }
}

impl ClientState {
    pub fn new_router(
        indexed_tx: flume::Sender<()>,
        notifier: flume::Sender<LspNotification>,
        project: PathBuf,
    ) -> Router<Self> {
        let mut router = Router::from_language_client(ClientState {
            indexed_tx: Some(indexed_tx),
            notifier,
            project,
        });
        router.event(Self::on_stop);
        router
    }

    pub fn on_stop(&mut self, _: Stop) -> ControlFlow<async_lsp::Result<()>> {
        ControlFlow::Break(Ok(()))
    }
}
