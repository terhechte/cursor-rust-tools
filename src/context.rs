use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::{
    lsp::RustAnalyzerLsp,
    project::{Project, TransportType},
};
use anyhow::Result;

/// Holds a project and its associated LSP instance
pub struct ProjectContext {
    pub project: Project,
    pub lsp: RustAnalyzerLsp,
}

#[derive(Clone)]
pub struct Context {
    projects: Arc<RwLock<HashMap<PathBuf, Arc<ProjectContext>>>>,
    transport: TransportType,
}

impl Context {
    pub fn new() -> Self {
        Self {
            projects: Arc::new(RwLock::new(HashMap::new())),
            transport: TransportType::Sse {
                host: "localhost".to_string(),
                port: 8080,
            },
        }
    }

    pub fn transport(&self) -> &TransportType {
        &self.transport
    }

    /// Add a new project to the context
    pub async fn add_project(&mut self, project: Project) -> Result<()> {
        let root = project.root().clone();
        let lsp = RustAnalyzerLsp::new(&project).await?;
        let project_context = Arc::new(ProjectContext { project, lsp });

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
