use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock, RwLockWriteGuard};

use crate::docs::{Docs, DocsNotification};
use crate::lsp::LspNotification;
use crate::mcp::McpNotification;
use crate::{
    lsp::RustAnalyzerLsp,
    project::{Project, TransportType},
};
use anyhow::Result;
use flume::Sender;

pub enum ContextNotification {
    Lsp(LspNotification),
    Docs(DocsNotification),
    Mcp(McpNotification),
}

pub struct ProjectContext {
    pub project: Project,
    pub lsp: RustAnalyzerLsp,
    pub docs: Docs,
    pub is_indexing_lsp: AtomicBool,
    pub is_indexing_docs: AtomicBool,
}

#[derive(Clone)]
pub struct Context {
    projects: Arc<RwLock<HashMap<PathBuf, Arc<ProjectContext>>>>,
    transport: TransportType,
    lsp_sender: Sender<LspNotification>,
    docs_sender: Sender<DocsNotification>,
    mcp_sender: Sender<McpNotification>,
}

impl Context {
    pub fn new(port: u16, notifier: Sender<ContextNotification>) -> Self {
        let (lsp_sender, lsp_receiver) = flume::unbounded();
        let (docs_sender, docs_receiver) = flume::unbounded();
        let (mcp_sender, mcp_receiver) = flume::unbounded();

        let projects = Arc::new(RwLock::new(HashMap::new()));

        let cloned_projects = projects.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(notification) = mcp_receiver.recv_async() => {
                        notifier.send(ContextNotification::Mcp(notification));
                    }
                    Ok(ref notification @ DocsNotification::Indexing { ref project, is_indexing }) = docs_receiver.recv_async() => {
                        notifier.send(ContextNotification::Docs(notification.clone()));
                        let mut projects: RwLockWriteGuard<'_, HashMap<PathBuf, Arc<ProjectContext>>> = cloned_projects.write().unwrap();
                        if let Some(project) = projects.get_mut(project) {
                            project.is_indexing_docs.store(is_indexing, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    Ok(ref notification @ LspNotification::Indexing { ref project, is_indexing }) = lsp_receiver.recv_async() => {
                        notifier.send(ContextNotification::Lsp(notification.clone()));
                        let mut projects: RwLockWriteGuard<'_, HashMap<PathBuf, Arc<ProjectContext>>> = cloned_projects.write().unwrap();
                        if let Some(project) = projects.get_mut(project) {
                            project.is_indexing_lsp.store(is_indexing, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
            }
        });

        Self {
            projects,
            transport: TransportType::Sse {
                host: "localhost".to_string(),
                port,
            },
            lsp_sender,
            docs_sender,
            mcp_sender,
        }
    }

    pub fn transport(&self) -> &TransportType {
        &self.transport
    }

    pub async fn send_mcp_notification(&self, notification: McpNotification) -> Result<()> {
        self.mcp_sender.send(notification)?;
        Ok(())
    }

    /// Add a new project to the context
    pub async fn add_project(&self, project: Project) -> Result<()> {
        let root = project.root().clone();
        let lsp = RustAnalyzerLsp::new(&project, self.lsp_sender.clone()).await?;
        let docs = Docs::new(project.clone(), self.docs_sender.clone())?;
        let project_context = Arc::new(ProjectContext {
            project,
            lsp,
            docs,
            is_indexing_lsp: AtomicBool::new(false),
            is_indexing_docs: AtomicBool::new(false),
        });

        let mut projects_map = self
            .projects
            .write()
            .expect("Failed to acquire write lock on projects");
        projects_map.insert(root, project_context);

        Ok(())
    }

    /// Remove a project from the context
    pub fn remove_project(&mut self, root: &PathBuf) -> Option<Arc<ProjectContext>> {
        let mut projects_map = self
            .projects
            .write()
            .expect("Failed to acquire write lock on projects");
        projects_map.remove(root)
    }

    /// Get a reference to a project context by its root path
    pub fn get_project(&self, root: &PathBuf) -> Option<Arc<ProjectContext>> {
        let projects_map = self
            .projects
            .read()
            .expect("Failed to acquire read lock on projects");
        projects_map.get(root).cloned()
    }

    /// Get a reference to a project context by any path within the project
    /// Will traverse up the path hierarchy until it finds a matching project root
    pub fn get_project_by_path(&self, path: &PathBuf) -> Option<Arc<ProjectContext>> {
        let mut current_path = path.clone();

        let projects_map = self
            .projects
            .read()
            .expect("Failed to acquire read lock on projects");

        if let Some(project) = projects_map.get(&current_path) {
            return Some(project.clone());
        }

        while let Some(parent) = current_path.parent() {
            current_path = parent.to_path_buf();
            if let Some(project) = projects_map.get(&current_path) {
                return Some(project.clone());
            }
        }

        None
    }
}
