use std::ops::ControlFlow;
use std::path::PathBuf;

use super::Stop;
use crate::lsp::LspNotification;
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
            matches!(params.token, NumberOrString::String(s) if RA_INDEXING_TOKENS.contains(&&*s));
        let is_work_done = matches!(
            params.value,
            ProgressParamsValue::WorkDone(WorkDoneProgress::End(_))
        );
        if is_indexing && !is_work_done {
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
        if is_indexing && is_work_done {
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

            if let Some(tx) = &self.indexed_tx {
                if let Err(e) = tx.try_send(()) {
                    if matches!(e, flume::TrySendError::Disconnected(_)) {
                        tracing::debug!("Channel closed when sending indexing completion: {}", e);
                    } else {
                        tracing::error!("Failed to send indexing completion signal: {}", e);
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
