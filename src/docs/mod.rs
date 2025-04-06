use std::{path::PathBuf, sync::Arc};

use flume::Sender;
use generate::generate_docs;
use tokio::sync::Mutex;
use walk::walk_docs;

use crate::project::Project;
use anyhow::Result;

pub mod extract_md;
pub mod generate;
pub mod index;
pub mod utils;
pub mod walk;

#[derive(Debug, Clone)]
pub enum DocsNotification {
    Indexing { project: PathBuf, is_indexing: bool },
}

#[derive(Debug)]
pub struct Docs {
    project: Project,
    index: Arc<Mutex<index::DocsIndex>>,
    notifier: Sender<DocsNotification>,
}

impl Docs {
    pub fn new(project: Project, notifier: Sender<DocsNotification>) -> Result<Self> {
        let index = Mutex::new(index::DocsIndex::new(&project)?);
        Ok(Self {
            project,
            index: Arc::new(index),
            notifier,
        })
    }

    pub async fn update_index(&self) -> Result<()> {
        self.notifier.send(DocsNotification::Indexing {
            project: self.project.root().to_path_buf(),
            is_indexing: true,
        })?;
        let cloned_project = self.project.clone();
        let cloned_index = self.index.clone();
        let cloned_notifier = self.notifier.clone();
        tokio::spawn(async move {
            if let Err(e) = generate_docs(&cloned_project) {
                tracing::error!("Failed to generate docs: {:?}", e);
            }
            if let Err(e) = walk_docs(&cloned_project) {
                tracing::error!("Failed to update docs cache: {:?}", e);
            }

            tracing::info!("Updating docs cache...");

            let index = match index::DocsIndex::new(&cloned_project) {
                Ok(index) => index,
                Err(e) => {
                    tracing::error!("Failed to update docs cache: {:?}", e);
                    if let Err(e) = cloned_notifier.send(DocsNotification::Indexing {
                        project: cloned_project.root().to_path_buf(),
                        is_indexing: false,
                    }) {
                        tracing::error!("Failed to send docs indexing notification: {:?}", e);
                    }
                    return;
                }
            };
            *cloned_index.lock().await = index;

            if let Err(e) = cloned_notifier.send(DocsNotification::Indexing {
                project: cloned_project.root().to_path_buf(),
                is_indexing: false,
            }) {
                tracing::error!("Failed to send docs indexing notification: {:?}", e);
            }
        });
        Ok(())
    }

    pub async fn crate_docs(&self, crate_name: &str) -> Result<String> {
        let index = self.index.lock().await;
        if index.dependencies().is_empty() {
            return Err(anyhow::anyhow!(
                "No dependencies found. Please update the docs cache first"
            ));
        }
        let markdown = index.markdown_docs(&crate_name).unwrap();
        Ok(markdown)
    }

    pub async fn crate_symbol_docs(
        &self,
        crate_name: &str,
        symbol: &str,
    ) -> Result<Vec<(String, String)>> {
        let index = self.index.lock().await;
        if index.dependencies().is_empty() {
            return Err(anyhow::anyhow!(
                "No dependencies found. Please update the docs cache first"
            ));
        }
        let Some(docs) = index.docs(crate_name, &[symbol.to_string()]) else {
            return Err(anyhow::anyhow!("No docs found for crate: {}", crate_name));
        };
        Ok(docs)
    }
}
