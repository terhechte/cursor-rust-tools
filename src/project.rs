use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportType {
    Stdio,
    Sse { host: String, port: u16 },
}

#[derive(Clone, Debug)]
pub struct Project {
    pub root: PathBuf,
    pub ignore_crates: Vec<String>,
}

impl Project {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        Ok(Self {
            root,
            ignore_crates: vec![],
        })
    }

    pub fn ignore_crates(&self) -> &[String] {
        &self.ignore_crates
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    pub fn uri(&self) -> Result<Url> {
        Url::from_file_path(&self.root)
            .map_err(|_| anyhow::anyhow!("Failed to create project root URI"))
    }

    pub fn docs_dir(&self) -> PathBuf {
        self.cache_dir().join("doc")
    }

    pub fn cache_folder(&self) -> &str {
        ".docs-cache"
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join(self.cache_folder())
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
