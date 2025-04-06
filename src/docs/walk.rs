use anyhow::Result;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self};
use std::path::{Path, PathBuf};

use super::extract_md::extract_md;
use super::utils::{get_cargo_dependencies, parse_rust_symbol};

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct DocsCache {
    pub hash: String,
    pub deps: HashMap<String, HashMap<String, String>>,
    pub crate_versions: HashMap<String, String>,
}

impl DocsCache {
    pub fn new(project: &crate::project::Project) -> Result<Self> {
        let cache_path = project.cache_dir().join("docs_cache.json");
        if cache_path.exists() {
            let content = fs::read_to_string(cache_path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, project: &crate::project::Project) -> Result<()> {
        let cache_path = project.cache_dir().join("docs_cache.json");
        fs::create_dir_all(project.cache_dir())?;
        fs::write(cache_path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

pub fn walk_docs(project: &crate::project::Project) -> Result<()> {
    let mut cache = DocsCache::new(project)?;

    let dependencies = get_cargo_dependencies(project)?;
    tracing::info!("dependencies: {:?}", dependencies);

    // Convert dependencies to a HashMap for easier lookup
    let dep_versions: HashMap<String, String> = dependencies.into_iter().collect();

    // Walk the docs directory
    let walker = WalkBuilder::new(project.docs_dir()).hidden(false).build();

    for result in walker {
        let entry = result?;
        let path = entry.path();

        if path.extension().and_then(|ext| ext.to_str()) == Some("html") {
            if let Some(relative_path) = path_to_cache_key(path, project.docs_dir()) {
                if let Some((crate_name, file_path)) = extract_crate_and_path(&relative_path) {
                    // Skip if crate is not in dependencies
                    let Some(version) = dep_versions.get(crate_name) else {
                        tracing::debug!(
                            "Skipping {crate_name}: {file_path} because it's not in dependencies"
                        );
                        continue;
                    };

                    // Skip if crate is in ignore list
                    if project.ignore_crates().contains(&crate_name.to_string()) {
                        tracing::debug!("Skipping {crate_name} because it's in ignore list");
                        continue;
                    }

                    // Skip if version hasn't changed
                    if let Some(cached_version) = cache.crate_versions.get(crate_name) {
                        if cached_version == version {
                            tracing::debug!(
                                "Skipping {crate_name} because the version has not changed"
                            );
                            continue;
                        }
                    }

                    // Process the file since it's either new or updated
                    let html_content = fs::read_to_string(path)?;
                    let markdown = extract_md(&html_content);
                    tracing::debug!("Indexing {crate_name}: {file_path}");

                    let symbol = parse_rust_symbol(file_path)
                        .map(|s| s.to_string())
                        .unwrap_or(file_path.to_string());

                    cache
                        .deps
                        .entry(crate_name.to_string())
                        .or_default()
                        .insert(symbol, markdown);

                    // Store the version number
                    cache
                        .crate_versions
                        .insert(crate_name.to_string(), version.clone());
                }
            }
        }
    }

    // Create and save cache
    cache.save(project)?;

    Ok(())
}

fn path_to_cache_key(path: &Path, docs_dir: PathBuf) -> Option<String> {
    path.strip_prefix(docs_dir)
        .ok()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string())
}

fn extract_crate_and_path(path: &str) -> Option<(&str, &str)> {
    println!("path: {path}");
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    match parts.as_slice() {
        [crate_name, rest] => Some((*crate_name, *rest)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::project::Project;

    use super::*;

    #[test]
    fn test_walk_docs() {
        // let (repository, guard) = crate::test_utils::test_repository();
        let project = Project::new(PathBuf::from("assets/zoxide-main")).unwrap();
        walk_docs(&project).unwrap();
        // guard.keep();
    }
}
