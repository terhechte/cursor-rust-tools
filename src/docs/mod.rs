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
    pub fn new(project: &Project, notifier: Sender<DocsNotification>) -> Result<Self> {
        // First check if the project directory exists
        if !project.root().exists() {
            return Err(anyhow::anyhow!(
                "Project root does not exist: {:?}",
                project.root()
            ));
        }

        // Try to create cache directory if it doesn't exist
        let cache_dir = project.cache_dir();
        if !cache_dir.exists() {
            match std::fs::create_dir_all(&cache_dir) {
                Ok(_) => tracing::debug!("Created cache directory: {:?}", cache_dir),
                Err(e) => {
                    tracing::error!(
                        "Failed to create docs cache directory at {:?}: {}",
                        cache_dir,
                        e
                    );
                    if cfg!(windows) {
                        tracing::error!("Windows path issue: Check if path contains special characters or spaces");
                    }
                    // Continue anyway, DocsIndex::new will also try to create the directory
                }
            }
        }

        // Try to initialize the index with robust error handling
        let index = match index::DocsIndex::new(project) {
            Ok(idx) => idx,
            Err(e) => {
                tracing::warn!("Failed to create docs index: {}", e);
                // Instead of creating a malformed index directly, let's create a fallback
                let cache_path = cache_dir.join("docs_cache.json");
                if !cache_path.exists() {
                    // Create an empty cache file
                    let empty_cache = walk::DocsCache::default();
                    if let Err(write_err) = std::fs::write(
                        &cache_path,
                        serde_json::to_string(&empty_cache).unwrap_or_default(),
                    ) {
                        tracing::error!("Failed to write empty cache file: {}", write_err);
                    }
                }
                // Try one more time with empty dependencies
                match index::DocsIndex::new(project) {
                    Ok(idx) => idx,
                    Err(e2) => {
                        tracing::error!("Still failed to create index on retry: {}", e2);
                        return Err(anyhow::anyhow!("Could not initialize docs index: {}", e));
                    }
                }
            }
        };

        Ok(Self {
            project: project.clone(),
            index: Arc::new(Mutex::new(index)),
            notifier,
        })
    }

    /// Create a minimal docs instance with an empty index for when normal initialization fails
    pub fn new_empty(project: &Project, notifier: Sender<DocsNotification>) -> Result<Self> {
        tracing::warn!("Creating minimal docs client with empty index");
        
        // Use the new_empty constructor for DocsIndex
        let index = index::DocsIndex::new_empty();
        
        Ok(Self {
            project: project.clone(),
            index: Arc::new(Mutex::new(index)),
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
        let markdown = index.markdown_docs(crate_name).unwrap();
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
