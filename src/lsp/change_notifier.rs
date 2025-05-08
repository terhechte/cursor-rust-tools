use std::{path::PathBuf, sync::Arc, time::Duration};
use anyhow::Result;
use async_lsp::{LanguageServer, ServerSocket};
use lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent};
use notify_debouncer_mini::{
    DebounceEventResult, DebouncedEvent, Debouncer, new_debouncer, notify::*,
};
use tokio::{runtime::Handle, sync::Mutex};
use url::Url;
use crate::project::Project;

#[derive(Debug)]
pub struct ChangeNotifier {
    #[allow(dead_code)] // Keep the handle to ensure the change notifier runs
    debouncer: Debouncer<RecommendedWatcher>,
}

impl ChangeNotifier {
    pub fn new(
        server: Arc<Mutex<ServerSocket>>,
        project: &Project,
        handle: Handle,
    ) -> Result<Self> {
        let handle_clone = handle.clone();
        let target_path = project.root().join("target");
        let mut debouncer = new_debouncer(
            Duration::from_secs(2),
            move |res: DebounceEventResult| match res {
                Ok(events) => events.iter().for_each(|e| {
                    handle_event(e, server.clone(), handle_clone.clone(), target_path.clone())
                }),
                Err(e) => tracing::error!("Error {:?}", e),
            },
        )?;
        // We watch the root folder
        debouncer
            .watcher()
            .watch(project.root(), RecursiveMode::Recursive)?;
        Ok(Self { debouncer })
    }
}

fn handle_event(
    event: &DebouncedEvent,
    server: Arc<Mutex<ServerSocket>>,
    handle: Handle,
    target_path: PathBuf,
) {
    // Don't trigger lsp on target files. Otherwise it will trigger itself.
    if event.path.starts_with(&target_path) {
        return;
    }
    tracing::trace!("Event {:?} for {:?}", event.kind, event.path);
    let url = match Url::from_file_path(event.path.clone()) {
        Ok(url) => url,
        Err(e) => {
            tracing::error!("Failed to convert file path to URL: {:?}", e);
            return;
        }
    };
    handle.spawn(async move {
        match server
            .lock()
            .await
            .did_change_watched_files(DidChangeWatchedFilesParams {
                changes: vec![FileEvent::new(url, FileChangeType::CHANGED)],
            }) {
            Ok(_) => (),
            Err(e) => tracing::error!("Failed to send DidChangeWatchedFiles notification: {:?}", e),
        }
    });
}
