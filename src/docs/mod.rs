use std::path::PathBuf;

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

pub struct Docs {
    project: Project,
    index: Mutex<index::DocsIndex>,
    notifier: Sender<DocsNotification>,
}

impl Docs {
    pub fn new(project: Project, notifier: Sender<DocsNotification>) -> Result<Self> {
        let index = Mutex::new(index::DocsIndex::new(&project)?);
        Ok(Self {
            project,
            index,
            notifier,
        })
    }

    pub async fn update_index(&self) -> Result<()> {
        self.notifier.send(DocsNotification::Indexing {
            project: self.project.root().to_path_buf(),
            is_indexing: true,
        });
        generate_docs(&self.project)?;
        println!("Updating docs cache...");
        if let Err(e) = walk_docs(&self.project) {
            return Err(anyhow::anyhow!("Failed to update docs cache: {:?}", e));
        }
        let index = index::DocsIndex::new(&self.project)?;
        *self.index.lock().await = index;
        self.notifier.send(DocsNotification::Indexing {
            project: self.project.root().to_path_buf(),
            is_indexing: false,
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
        let Some(docs) = index.docs(&crate_name, &[symbol.to_string()]) else {
            return Err(anyhow::anyhow!("No docs found for crate: {}", crate_name));
        };
        Ok(docs)
    }
}
