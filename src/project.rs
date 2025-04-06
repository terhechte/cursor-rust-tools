use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportType {
    Stdio,
    Sse { host: String, port: u16 },
}

#[derive(Clone)]
pub struct Project {
    root: PathBuf,
    transport: TransportType,
}

impl Project {
    pub fn new(root: impl AsRef<Path>, transport: TransportType) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        Ok(Self { root, transport })
    }

    pub fn transport(&self) -> &TransportType {
        &self.transport
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    pub fn uri(&self) -> Result<Url> {
        Url::from_file_path(&self.root)
            .map_err(|_| anyhow::anyhow!("Failed to create project root URI"))
    }

    pub fn file_uri(&self, relative_path: impl AsRef<Path>) -> Result<Url> {
        Url::from_file_path(self.root.join(relative_path))
            .map_err(|_| anyhow::anyhow!("Failed to create file URI"))
    }

    /// Given an absolute path, return the path relative to the project root.
    /// Returns an error if the path is not within the project root.
    pub fn relative_path(&self, absolute_path: impl AsRef<Path>) -> Result<String, String> {
        let absolute_path = absolute_path.as_ref();
        absolute_path
            .strip_prefix(&self.root)
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|_| {
                format!(
                    "Path {:?} is not inside project root {:?}",
                    absolute_path, self.root
                )
            })
    }
}
