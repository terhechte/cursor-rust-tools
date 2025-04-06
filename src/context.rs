use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock, RwLockWriteGuard};

use crate::docs::{Docs, DocsNotification};
use crate::lsp::LspNotification;
use crate::mcp::McpNotification;
use crate::ui::ProjectDescription;
use crate::{
    lsp::RustAnalyzerLsp,
    project::{Project, TransportType},
};
use anyhow::Result;
use flume::Sender;

#[derive(Debug)]
pub enum ContextNotification {
    Lsp(LspNotification),
    Docs(DocsNotification),
    Mcp(McpNotification),
    ProjectAdded(PathBuf),
    ProjectRemoved(PathBuf),
}

impl ContextNotification {
    pub fn project_name(&self) -> String {
        let project_path = match self {
            ContextNotification::Lsp(LspNotification::Indexing { project, .. }) => project.clone(),
            ContextNotification::Docs(DocsNotification::Indexing { project, .. }) => {
                project.clone()
            }
            ContextNotification::Mcp(McpNotification::Request { project, .. }) => project.clone(),
            ContextNotification::Mcp(McpNotification::Response { project, .. }) => project.clone(),
            ContextNotification::ProjectAdded(project) => project.clone(),
            ContextNotification::ProjectRemoved(project) => project.clone(),
        };
        project_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string()
    }
}

#[derive(Debug)]
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
    notifier: Sender<ContextNotification>,
}

impl Context {
    pub fn new(port: u16, notifier: Sender<ContextNotification>) -> Self {
        let (lsp_sender, lsp_receiver) = flume::unbounded();
        let (docs_sender, docs_receiver) = flume::unbounded();
        let (mcp_sender, mcp_receiver) = flume::unbounded();

        let projects = Arc::new(RwLock::new(HashMap::new()));

        let cloned_projects = projects.clone();
        let cloned_notifier = notifier.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(notification) = mcp_receiver.recv_async() => {
                        if let Err(e) = cloned_notifier.send(ContextNotification::Mcp(notification)) {
                            tracing::error!("Failed to send MCP notification: {}", e);
                        }
                    }
                    Ok(ref notification @ DocsNotification::Indexing { ref project, is_indexing }) = docs_receiver.recv_async() => {
                        if let Err(e) = cloned_notifier.send(ContextNotification::Docs(notification.clone())) {
                            tracing::error!("Failed to send docs notification: {}", e);
                        }
                        let mut projects: RwLockWriteGuard<'_, HashMap<PathBuf, Arc<ProjectContext>>> = cloned_projects.write().unwrap();
                        if let Some(project) = projects.get_mut(project) {
                            project.is_indexing_docs.store(is_indexing, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    Ok(ref notification @ LspNotification::Indexing { ref project, is_indexing }) = lsp_receiver.recv_async() => {
                        if let Err(e) = cloned_notifier.send(ContextNotification::Lsp(notification.clone())) {
                            tracing::error!("Failed to send LSP notification: {}", e);
                        }
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
            notifier,
        }
    }

    pub fn project_descriptions(&self) -> Vec<ProjectDescription> {
        let projects_map = self
            .projects
            .read()
            .expect("Failed to acquire read lock on projects");
        projects_map
            .values()
            .map(|project| ProjectDescription {
                root: project.project.root().clone(),
                name: project
                    .project
                    .root()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                is_indexing_lsp: project
                    .is_indexing_lsp
                    .load(std::sync::atomic::Ordering::Relaxed),
                is_indexing_docs: project
                    .is_indexing_docs
                    .load(std::sync::atomic::Ordering::Relaxed),
            })
            .collect()
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
        docs.update_index().await?;

        let project_context = Arc::new(ProjectContext {
            project,
            lsp,
            docs,
            is_indexing_lsp: AtomicBool::new(true),
            is_indexing_docs: AtomicBool::new(true),
        });

        let mut projects_map = self
            .projects
            .write()
            .expect("Failed to acquire write lock on projects");
        projects_map.insert(root.clone(), project_context);
        if let Err(e) = self.notifier.send(ContextNotification::ProjectAdded(root)) {
            tracing::error!("Failed to send project added notification: {}", e);
        }
        Ok(())
    }

    /// Remove a project from the context
    pub fn remove_project(&mut self, root: &PathBuf) -> Option<Arc<ProjectContext>> {
        let mut projects_map = self
            .projects
            .write()
            .expect("Failed to acquire write lock on projects");
        let project = projects_map.remove(root);
        if let Err(e) = self
            .notifier
            .send(ContextNotification::ProjectRemoved(root.clone()))
        {
            tracing::error!("Failed to send project removed notification: {}", e);
        }
        project
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
    pub fn get_project_by_path(&self, path: &Path) -> Option<Arc<ProjectContext>> {
        let mut current_path = path.to_path_buf();

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

    pub async fn force_index_docs(&self, project: &PathBuf) -> Result<()> {
        let project_context = self.get_project(project).unwrap();
        // project_context.docs.update_index().await?;
        let oldval = project_context
            .is_indexing_docs
            .load(std::sync::atomic::Ordering::Relaxed);
        project_context
            .is_indexing_docs
            .store(!oldval, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
}
