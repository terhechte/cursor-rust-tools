use super::{utils::get_cargo_dependencies, walk::DocsCache};
use anyhow::Result;
use std::fs;

#[derive(Debug)]
pub struct DocsIndex {
    dependencies: Vec<(String, String)>,
    cache: DocsCache,
}

impl DocsIndex {
    pub fn new(repository: &crate::project::Project) -> Result<Self> {
        let dependencies = get_cargo_dependencies(repository)?;

        // Try to create cache directory with better error handling
        let cache_dir = repository.cache_dir();
        if !cache_dir.exists() {
            // On Windows, try to create the cache directory with explicit error tracing
            if let Err(e) = fs::create_dir_all(&cache_dir) {
                tracing::error!("Failed to create cache directory at {:?}: {}", cache_dir, e);
                // Attempt to provide more details about the error on Windows
                if cfg!(windows) {
                    tracing::error!("Windows error details: Path might contain special characters or require permissions.");
                    tracing::error!("Cache directory path: {:?}", cache_dir);
                }
                return Err(anyhow::anyhow!("Failed to create cache directory: {}", e));
            }
            // Verify the directory was actually created
            if !cache_dir.exists() {
                tracing::error!("Cache directory still doesn't exist after creation attempt: {:?}", cache_dir);
                return Err(anyhow::anyhow!("Failed to verify cache directory creation"));
            }
        }

        // Read or create cache file with better error handling
        let cache_path = cache_dir.join("docs_cache.json");
        
        let cache = if !cache_path.exists() {
            tracing::info!("Creating new docs cache at {:?}", cache_path);
            let cache = DocsCache::default();
            
            // Write the cache file with error handling
            match serde_json::to_string(&cache) {
                Ok(cache_content) => {
                    if let Err(e) = fs::write(&cache_path, cache_content) {
                        tracing::error!("Failed to write cache file at {:?}: {}", cache_path, e);
                        return Err(anyhow::anyhow!("Failed to write cache file: {}", e));
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to serialize cache to JSON: {}", e);
                    return Err(anyhow::anyhow!("Failed to serialize cache: {}", e));
                }
            };
            
            cache
        } else {
            // Read existing cache file
            match fs::read_to_string(&cache_path) {
                Ok(cache_content) => {
                    match serde_json::from_str(&cache_content) {
                        Ok(parsed_cache) => parsed_cache,
                        Err(e) => {
                            tracing::error!("Failed to parse cache file as JSON: {}", e);
                            return Err(anyhow::anyhow!("Failed to parse cache file: {}", e));
                        }
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to read cache file at {:?}: {}", cache_path, e);
                    return Err(anyhow::anyhow!("Failed to read cache file: {}", e));
                }
            }
        };

        Ok(DocsIndex {
            dependencies,
            cache,
        })
    }
    
    /// Create an empty DocsIndex without requiring file operations.
    /// Used as a fallback when normal initialization fails.
    pub fn new_empty() -> Self {
        DocsIndex {
            dependencies: Vec::new(),
            cache: DocsCache::default(),
        }
    }

    pub fn dependencies(&self) -> &[(String, String)] {
        &self.dependencies
    }

    pub fn symbols(&self, dependency: &str) -> Option<Vec<String>> {
        self.cache
            .deps
            .get(dependency)
            .map(|symbols| symbols.keys().cloned().collect())
    }

    pub fn docs(&self, dependency: &str, symbols: &[String]) -> Option<Vec<(String, String)>> {
        let dep_docs = self.cache.deps.get(dependency)?;
        Some(
            symbols
                .iter()
                .filter_map(|symbol| {
                    let doc = dep_docs.get(symbol)?;
                    Some((symbol.clone(), doc.clone()))
                })
                .collect(),
        )
    }

    pub fn markdown_docs(&self, dependency: &str) -> Option<String> {
        let mut output = String::new();

        let symbols = self.symbols(dependency)?;
        for symbol in symbols {
            if let Some(docs) = self.docs(dependency, &[symbol.clone()]) {
                output.push_str(&symbol);
                output.push('\n');
                for (doc, content) in docs {
                    output.push_str(&doc);
                    output.push('\n');
                    output.push_str(&content);
                    output.push('\n');
                }
            }
        }
        Some(output)
    }
}
