use std::collections::HashMap;
use std::fs;
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
use serde::{Deserialize, Serialize};
use tokio::task;

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

const HOSTNAME: &str = "localhost";
const CONFIGURATION_FILE: &str = ".cursor-rust-tools";

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
    pub async fn new(port: u16, notifier: Sender<ContextNotification>) -> Self {
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

        let context = Self {
            projects,
            transport: TransportType::Sse {
                host: HOSTNAME.to_string(),
                port,
            },
            lsp_sender,
            docs_sender,
            mcp_sender,
            notifier,
        };

        // Load config after initial setup
        // let cloned_context = context.clone();
        // task::spawn(async move {
        //     if let Err(e) = cloned_context.load_config().await {
        //         tracing::error!("Failed to load config on startup: {}", e);
        //     }
        // });

        context
    }

    pub fn address_information(&self) -> (String, u16) {
        match &self.transport {
            TransportType::Stdio => ("stdio".to_string(), 0),
            TransportType::Sse { host, port } => (host.clone(), *port),
        }
    }

    pub fn mcp_configuration(&self) -> String {
        let (host, port) = self.address_information();
        CONFIG_TEMPLATE
            .replace("{{HOST}}", &host)
            .replace("{{PORT}}", &port.to_string())
    }

    pub fn configuration_file(&self) -> String {
        format!("~/{}", CONFIGURATION_FILE)
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

    fn config_path(&self) -> PathBuf {
        let parsed = shellexpand::tilde(&self.configuration_file()).to_string();
        PathBuf::from(parsed)
    }

    fn write_config(&self) -> Result<()> {
        println!("write config");
        let projects_map = self
            .projects
            .read()
            .expect("Failed to acquire read lock on projects");
        let projects_to_save: Vec<&Project> = projects_map.values().map(|pc| &pc.project).collect();
        println!("gotten info config");

        let config_path = self.config_path();

        let toml_string = toml::to_string_pretty(&projects_to_save)?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&config_path, toml_string)?;
        tracing::debug!("Wrote config file to {:?}", config_path);
        Ok(())
    }

    pub async fn load_config(&self) -> Result<()> {
        let config_path = self.config_path();

        if !config_path.exists() {
            tracing::info!(
                "Configuration file not found at {:?}, skipping load.",
                config_path
            );
            return Ok(());
        }

        let toml_string = match fs::read_to_string(&config_path) {
            Ok(content) => content,
            Err(e) => {
                tracing::error!("Failed to read config file {:?}: {}", config_path, e);
                return Err(e.into()); // Propagate read error
            }
        };

        if toml_string.trim().is_empty() {
            tracing::info!(
                "Configuration file {:?} is empty, skipping load.",
                config_path
            );
            return Ok(());
        }

        let loaded_projects: Vec<Project> = match toml::from_str(&toml_string) {
            Ok(projects) => projects,
            Err(e) => {
                tracing::error!(
                    "Failed to parse TOML from config file {:?}: {}",
                    config_path,
                    e
                );
                // Don't return error here, maybe the file is corrupt but we can continue
                return Ok(());
            }
        };

        for project in loaded_projects {
            // Validate project root before adding
            if !project.root().exists() || !project.root().is_dir() {
                tracing::warn!(
                    "Project root {:?} from config does not exist or is not a directory, skipping.",
                    project.root()
                );
                continue;
            }
            // We need to canonicalize again as the stored path might be relative or different
            match Project::new(project.root()) {
                Ok(new_project) => {
                    if let Err(e) = self.add_project(new_project).await {
                        tracing::error!(
                            "Failed to add project {:?} from config: {}",
                            project.root(),
                            e
                        );
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to create project for root {:?} from config: {}",
                        project.root(),
                        e
                    );
                }
            }
        }

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

        drop(projects_map);

        // Write config after successfully adding
        if let Err(e) = self.write_config() {
            tracing::error!("Failed to write config after adding project: {}", e);
        }
        Ok(())
    }

    /// Remove a project from the context
    pub fn remove_project(&mut self, root: &PathBuf) -> Option<Arc<ProjectContext>> {
        let project = {
            let mut projects_map = self
                .projects
                .write()
                .expect("Failed to acquire write lock on projects");
            projects_map.remove(root)
        };

        if project.is_some() {
            if let Err(e) = self
                .notifier
                .send(ContextNotification::ProjectRemoved(root.clone()))
            {
                tracing::error!("Failed to send project removed notification: {}", e);
            }
            // Write config after successfully removing
            if let Err(e) = self.write_config() {
                tracing::error!("Failed to write config after removing project: {}", e);
            }
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

const CONFIG_TEMPLATE: &str = r#"
{
    "mcpServers": {
        "server-name": {
            "url": "http://{{HOST}}:{{PORT}}/sse",
            "env": {
                "API_KEY": ""
            }
        }
    }
}
"#;
