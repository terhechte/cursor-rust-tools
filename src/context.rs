use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::{RwLock, RwLockWriteGuard};

use crate::cargo_remote::CargoRemote;
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

#[derive(Debug, Clone)]
pub enum ContextNotification {
    Lsp(LspNotification),
    Docs(DocsNotification),
    Mcp(McpNotification),
    ProjectAdded(PathBuf),
    ProjectRemoved(PathBuf),
    ProjectDescriptions(Vec<ProjectDescription>),
}

impl ContextNotification {
    pub fn notification_path(&self) -> PathBuf {
        match self {
            ContextNotification::Lsp(LspNotification::Indexing { project, .. }) => project.clone(),
            ContextNotification::Lsp(LspNotification::IndexingProgress(progress)) => progress.project.clone(),
            ContextNotification::Lsp(LspNotification::IndexingPauseResume { project, .. }) => project.clone(),
            ContextNotification::Docs(DocsNotification::Indexing { project, .. }) => {
                project.clone()
            }
            ContextNotification::Mcp(McpNotification::Request { project, .. }) => project.clone(),
            ContextNotification::Mcp(McpNotification::Response { project, .. }) => project.clone(),
            ContextNotification::ProjectAdded(project) => project.clone(),
            ContextNotification::ProjectRemoved(project) => project.clone(),
            ContextNotification::ProjectDescriptions(_) => PathBuf::from("project_descriptions"),
        }
    }

    pub fn description(&self) -> String {
        match self {
            ContextNotification::Lsp(LspNotification::Indexing { is_indexing, .. }) => {
                format!(
                    "LSP Indexing: {}",
                    if *is_indexing { "Started" } else { "Finished" }
                )
            }
            ContextNotification::Lsp(LspNotification::IndexingProgress(progress)) => {
                format!("LSP Indexing: {}", progress.status_message())
            }
            ContextNotification::Lsp(LspNotification::IndexingPauseResume { should_pause, .. }) => {
                format!(
                    "LSP Indexing: {}",
                    if *should_pause { "Paused" } else { "Resumed" }
                )
            }
            ContextNotification::Docs(DocsNotification::Indexing { is_indexing, .. }) => {
                format!(
                    "Docs Indexing: {}",
                    if *is_indexing { "Started" } else { "Finished" }
                )
            }
            ContextNotification::Mcp(McpNotification::Request { content, .. }) => {
                format!("MCP Request: {:?}", content)
            }
            ContextNotification::Mcp(McpNotification::Response { content, .. }) => {
                format!("MCP Response: {:?}", content)
            }
            ContextNotification::ProjectAdded(project) => {
                format!("Project Added: {:?}", project)
            }
            ContextNotification::ProjectRemoved(project) => {
                format!("Project Removed: {:?}", project)
            }
            ContextNotification::ProjectDescriptions(_) => "Project Descriptions".to_string(),
        }
    }
}

const HOSTNAME: &str = "localhost";
const CONFIGURATION_FILE: &str = ".cursor-rust-tools";

#[derive(Debug)]
pub struct ProjectContext {
    pub project: Project,
    pub lsp: RustAnalyzerLsp,
    pub docs: Docs,
    pub cargo_remote: CargoRemote,
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
                        if let Err(e) = cloned_notifier.try_send(ContextNotification::Mcp(notification)) {
                            if matches!(e, flume::TrySendError::Disconnected(_)) {
                                tracing::debug!("Channel closed when forwarding MCP notification");
                                break; // Exit the loop if the channel is disconnected
                            } else {
                                tracing::error!("Failed to send MCP notification: {}", e);
                            }
                        }
                    }
                    Ok(ref notification @ DocsNotification::Indexing { ref project, is_indexing }) = docs_receiver.recv_async() => {
                        if let Err(e) = cloned_notifier.try_send(ContextNotification::Docs(notification.clone())) {
                            if matches!(e, flume::TrySendError::Disconnected(_)) {
                                tracing::debug!("Channel closed when forwarding Docs notification");
                                break; // Exit the loop if the channel is disconnected
                            } else {
                                tracing::error!("Failed to send docs notification: {}", e);
                            }
                        }
                        let mut projects: RwLockWriteGuard<'_, HashMap<PathBuf, Arc<ProjectContext>>> = cloned_projects.write().await;
                        if let Some(project) = projects.get_mut(project) {
                            project.is_indexing_docs.store(is_indexing, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    Ok(notification) = lsp_receiver.recv_async() => {
                        if let LspNotification::IndexingProgress(ref progress) = notification {
                            // Handle indexing progress notification
                            if let Err(e) = cloned_notifier.try_send(ContextNotification::Lsp(notification.clone())) {
                                if matches!(e, flume::TrySendError::Disconnected(_)) {
                                    tracing::debug!("Channel closed when forwarding LSP progress notification");
                                    break; // Exit the loop if the channel is disconnected
                                } else {
                                    tracing::error!("Failed to send LSP progress notification: {}", e);
                                }
                            }
                            
                            // Also update the atomic flag for backward compatibility
                            let mut projects: RwLockWriteGuard<'_, HashMap<PathBuf, Arc<ProjectContext>>> = cloned_projects.write().await;
                            if let Some(project) = projects.get_mut(&progress.project) {
                                project.is_indexing_lsp.store(progress.is_indexing, std::sync::atomic::Ordering::Relaxed);
                            }
                        } else if let LspNotification::Indexing { ref project, is_indexing } = notification {
                            // Handle legacy indexing notification
                            if let Err(e) = cloned_notifier.try_send(ContextNotification::Lsp(notification.clone())) {
                                if matches!(e, flume::TrySendError::Disconnected(_)) {
                                    tracing::debug!("Channel closed when forwarding LSP notification");
                                    break; // Exit the loop if the channel is disconnected
                                } else {
                                    tracing::error!("Failed to send LSP notification: {}", e);
                                }
                            }
                            let mut projects: RwLockWriteGuard<'_, HashMap<PathBuf, Arc<ProjectContext>>> = cloned_projects.write().await;
                            if let Some(project_ctx) = projects.get_mut(project) {
                                project_ctx.is_indexing_lsp.store(is_indexing, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    }
                    else => {
                        // All channels closed
                        tracing::debug!("All message channels closed, exiting notification loop");
                        break;
                    }
                }
            }
        });

        Self {
            projects,
            transport: TransportType::Sse {
                host: HOSTNAME.to_string(),
                port,
            },
            lsp_sender,
            docs_sender,
            mcp_sender,
            notifier,
        }
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

    pub fn configuration_file(&self, project_root: &Path) -> String {
        PathBuf::from(project_root).join(CONFIGURATION_FILE).to_string_lossy().into_owned()
    }

    pub async fn project_descriptions(&self) -> Vec<ProjectDescription> {
        let projects_map = self.projects.read().await;
        project_descriptions(&projects_map).await
    }

    pub fn transport(&self) -> &TransportType {
        &self.transport
    }

    pub async fn send_mcp_notification(&self, notification: McpNotification) -> Result<()> {
        self.mcp_sender.send(notification)?;
        Ok(())
    }

    fn config_path(&self, project_root: &Path) -> PathBuf {
        PathBuf::from(project_root).join(CONFIGURATION_FILE)
    }

    async fn write_config(&self, project_root: &Path) -> Result<()> {
        let projects_map = self.projects.read().await;
        let projects_to_save: Vec<SerProject> = projects_map
            .values()
            .map(|pc| &pc.project)
            .map(|p| SerProject {
                root: p.root().to_string_lossy().to_string().replace('\\', "/"),
                ignore_crates: p.ignore_crates().to_vec(),
            })
            .collect();
        let config = SerConfig {
            projects: projects_to_save,
        };

        let config_path = self.config_path(project_root);

        let toml_string = toml::to_string_pretty(&config)?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&config_path, toml_string)?;
        tracing::debug!("Wrote config file to {:?}", config_path);
        Ok(())
    }

    pub async fn load_config(&self, project_root: &Path) -> Result<()> {
        let config_path = self.config_path(project_root);

        if !config_path.exists() {
            tracing::warn!(
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
            tracing::warn!(
                "Configuration file {:?} is empty, skipping load.",
                config_path
            );
            return Ok(());
        }

        // First try to parse normally
        let loaded_config: SerConfig = match toml::from_str(&toml_string) {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!(
                    "Failed to parse TOML from config file {:?}: {}. Attempting to fix Windows paths...",
                    config_path,
                    e
                );
                
                // Try to fix Windows paths by escaping backslashes
                // This handles manually edited config files with Windows paths
                let fixed_toml = toml_string.replace("\\", "\\\\");
                match toml::from_str(&fixed_toml) {
                    Ok(config) => config,
                    Err(e) => {
                        tracing::error!(
                            "Failed to parse TOML after fixing paths in config file {:?}: {}",
                            config_path,
                            e
                        );
                        // Don't return error here, maybe the file is corrupt but we can continue
                        return Ok(());
                    }
                }
            }
        };
        
        for project in loaded_config.projects {
            let project = Project {
                // PathBuf automatically handles forward slashes correctly on all platforms
                root: PathBuf::from(&project.root),
                ignore_crates: project.ignore_crates,
            };
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
        
        // Validate the path exists and is valid
        if !root.exists() {
            return Err(anyhow::anyhow!("Project root does not exist: {:?}", root));
        }
        
        // Check if it's already in the map
        if self.projects.read().await.contains_key(&root) {
            return Err(anyhow::anyhow!("Project already exists"));
        }

        // Try to create the LSP client, with helpful Windows error messages
        let lsp = match RustAnalyzerLsp::new(&project, self.lsp_sender.clone()).await {
            Ok(lsp) => lsp,
            Err(e) => {
                if cfg!(windows) {
                    tracing::error!("Failed to initialize LSP for Windows path: {:?}", root);
                    tracing::error!("Windows paths may need special handling: {}", e);
                }
                return Err(anyhow::anyhow!("Failed to initialize LSP: {}", e));
            }
        };

        // Create the docs client - but don't fail if it can't be created
        let docs = match Docs::new(&project, self.docs_sender.clone()) {
            Ok(docs) => docs,
            Err(e) => {
                // Create a default Docs instance to avoid failing the entire project addition
                tracing::warn!("Failed to initialize Docs client: {}. Creating empty docs client.", e);
                
                // Try to create cache dir if needed
                let cache_dir = project.cache_dir();
                if !cache_dir.exists() {
                    let _ = std::fs::create_dir_all(&cache_dir);
                }
                
                // Create default docs to avoid stopping project setup
                match Docs::new_empty(&project, self.docs_sender.clone()) {
                    Ok(docs) => docs,
                    Err(e2) => {
                        tracing::error!("Failed to create fallback docs client: {}", e2);
                        return Err(anyhow::anyhow!("Failed to initialize Docs client: {}", e));
                    }
                }
            }
        };

        let cargo_remote = CargoRemote::default();

        // Insert the project context
        let context = Arc::new(ProjectContext {
            project,
            lsp,
            docs,
            cargo_remote,
            is_indexing_lsp: AtomicBool::new(false),
            is_indexing_docs: AtomicBool::new(false),
        });

        self.projects.write().await.insert(root.clone(), context);

        // Send a notification that the project has been added
        if let Err(e) = self.notifier.try_send(ContextNotification::ProjectAdded(root.clone())) {
            if matches!(e, flume::TrySendError::Disconnected(_)) {
                tracing::debug!("Channel closed when sending project added notification");
            } else {
                tracing::error!("Failed to send notification: {}", e);
                return Err(anyhow::anyhow!("Failed to send notification: {}", e));
            }
        }

        // Write config after successfully adding
        if let Err(e) = self.write_config(&root).await {
            tracing::error!("Failed to write config after adding project: {}", e);
        }

        // Send a notification with the updated project descriptions
        self.request_project_descriptions();

        Ok(())
    }

    /// Remove a project from the context
    pub async fn remove_project(&self, root: &PathBuf) -> Option<Arc<ProjectContext>> {
        let project = {
            let mut projects_map = self.projects.write().await;
            projects_map.remove(root)
        };

        if project.is_some() {
            if let Err(e) = self.notifier.try_send(ContextNotification::ProjectRemoved(root.clone())) {
                if matches!(e, flume::TrySendError::Disconnected(_)) {
                    tracing::debug!("Channel closed when sending project removed notification");
                } else {
                    tracing::error!("Failed to send project removed notification: {}", e);
                }
            }
            // Write config after successfully removing
            if let Err(e) = self.write_config(root).await {
                tracing::error!("Failed to write config after removing project: {}", e);
            }
        }
        project
    }

    pub fn request_project_descriptions(&self) {
        let projects = self.projects.clone();
        let notifier = self.notifier.clone();
        tokio::spawn(async move {
            let projects_map = projects.read().await;
            let project_descriptions = project_descriptions(&projects_map).await;
            if let Err(e) = notifier.try_send(ContextNotification::ProjectDescriptions(
                project_descriptions,
            )) {
                if matches!(e, flume::TrySendError::Disconnected(_)) {
                    tracing::debug!("Channel closed when sending project descriptions");
                } else {
                    tracing::error!("Failed to send project descriptions: {}", e);
                }
            }
        });
    }

    /// Get a reference to a project context by its root path
    pub async fn get_project(&self, root: &PathBuf) -> Option<Arc<ProjectContext>> {
        let projects_map = self.projects.read().await;
        projects_map.get(root).cloned()
    }

    /// Get a reference to a project context by any path within the project
    /// Will traverse up the path hierarchy until it finds a matching project root
    pub async fn get_project_by_path(&self, path: &Path) -> Option<Arc<ProjectContext>> {
        let mut current_path = path.to_path_buf();

        let projects_map = self.projects.read().await;

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

    /// Forces doc indexing for the given project
    pub async fn force_index_docs(&self, project: &PathBuf) -> Result<()> {
        let Some(_project_context) = self.get_project(project).await else {
            return Err(anyhow::anyhow!("Project not found"));
        };
        let oldval = _project_context
            .is_indexing_docs
            .load(std::sync::atomic::Ordering::Relaxed);
        _project_context
            .is_indexing_docs
            .store(!oldval, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Toggles pause/resume for the LSP indexing process
    pub async fn toggle_indexing_pause(&self, project: &PathBuf, should_pause: bool) -> Result<()> {
        // Get the project context
        let Some(_project_context) = self.get_project(project).await else {
            return Err(anyhow::anyhow!("Project not found"));
        };
        
        // Send the pause/resume notification
        self.lsp_sender.send(LspNotification::IndexingPauseResume {
            project: project.clone(),
            should_pause,
        })?;
        
        // Log the action
        tracing::info!(
            "Sent indexing {} command for project {:?}",
            if should_pause { "pause" } else { "resume" },
            project
        );
        
        Ok(())
    }

    pub async fn shutdown_all(&self) {
        let projects = self.projects.write().await;
        for p in projects.values() {
            if let Err(e) = p.lsp.shutdown().await {
                tracing::error!(
                    "Failed to shutdown LSP for project {:?}: {}",
                    p.project.root(),
                    e
                );
            }
        }
    }
}

const CONFIG_TEMPLATE: &str = r#"
{
    "mcpServers": {
        "cursor_rust_tools": {
            "url": "http://{{HOST}}:{{PORT}}/sse",
            "env": {
                "API_KEY": ""
            }
        }
    }
}
"#;

#[derive(Serialize, Deserialize, Debug)]
struct SerConfig {
    projects: Vec<SerProject>,
}

#[derive(Serialize, Deserialize, Debug)]
struct SerProject {
    // Paths are stored with forward slashes for cross-platform compatibility
    root: String,
    ignore_crates: Vec<String>,
}

async fn project_descriptions(
    projects: &HashMap<PathBuf, Arc<ProjectContext>>,
) -> Vec<ProjectDescription> {
    projects
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
