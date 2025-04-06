use super::{utils::get_cargo_dependencies, walk::DocsCache};
use anyhow::Result;
use std::fs;

pub struct DocsIndex {
    dependencies: Vec<(String, String)>,
    cache: DocsCache,
}

impl DocsIndex {
    pub fn new(repository: &crate::project::Project) -> Result<Self> {
        let dependencies = get_cargo_dependencies(repository)?;

        if !repository.cache_dir().exists() {
            fs::create_dir_all(repository.cache_dir())?;
        }

        // Read cache file
        let cache_path = repository.cache_dir().join("docs_cache.json");
        if !cache_path.exists() {
            let cache = DocsCache::default();
            let cache_content = serde_json::to_string(&cache)?;
            fs::write(cache_path.clone(), cache_content)?;
        }
        let cache_content = fs::read_to_string(cache_path)?;
        let cache: DocsCache = serde_json::from_str(&cache_content)?;

        Ok(DocsIndex {
            dependencies,
            cache,
        })
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

#[cfg(test)]
mod tests {
    use crate::project::Project;

    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_docs_index() {
        let project = Project::new(PathBuf::from("assets/zoxide-main")).unwrap();
        let index = DocsIndex::new(&project).unwrap();

        // Test dependencies
        assert!(!index.dependencies().is_empty());

        let mut output = String::new();

        // Assuming there's a dependency named "anyhow" in the project
        if let Some(symbols) = index.symbols("anyhow") {
            // assert!(!symbols.is_empty());
            for symbol in symbols {
                if let Some(docs) = index.docs("anyhow", &[symbol.clone()]) {
                    output.push_str(&symbol);
                    output.push_str("\n");
                    for (doc, content) in docs {
                        output.push_str(&doc);
                        output.push_str("\n");
                        output.push_str(&content);
                        output.push_str("\n");
                    }
                }
            }
            println!("{}", output);

            // Test docs for the first symbol
            // if let Some(docs) = index.docs("anyhow", &[symbols[0].clone()]) {
            //     assert!(!docs.is_empty());
            //     dbg!(&docs);
            // }
        }
    }
}
