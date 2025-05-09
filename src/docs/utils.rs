use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use toml::Value;

#[derive(Debug, PartialEq)]
pub enum RustSymbol<'a> {
    Function(&'a str),
    Macro(&'a str),
    Struct(&'a str),
    Trait(&'a str),
    Type(&'a str),
    Enum(&'a str),
}

impl RustSymbol<'_> {
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        match self {
            RustSymbol::Function(name) => format!("fn {}", name),
            RustSymbol::Macro(name) => format!("macro {}!", name),
            RustSymbol::Struct(name) => format!("struct {}", name),
            RustSymbol::Trait(name) => format!("trait {}", name),
            RustSymbol::Type(name) => format!("type {}", name),
            RustSymbol::Enum(name) => format!("enum {}", name),
        }
    }
}

pub fn parse_rust_symbol(filename: &str) -> Option<RustSymbol> {
    // Split on the first dot to separate the kind from the name
    let parts: Vec<&str> = filename.splitn(2, '.').collect();
    if parts.len() != 2 {
        return None;
    }

    let (kind, name) = (parts[0], parts[1]);

    // Remove .html extension if present
    let name = name.strip_suffix(".html").unwrap_or(name);

    // Remove ! from macro names if present
    let name = name.strip_suffix('!').unwrap_or(name);

    match kind {
        "fn" => Some(RustSymbol::Function(name)),
        "macro" => Some(RustSymbol::Macro(name)),
        "struct" => Some(RustSymbol::Struct(name)),
        "trait" => Some(RustSymbol::Trait(name)),
        "type" => Some(RustSymbol::Type(name)),
        "enum" => Some(RustSymbol::Enum(name)),
        _ => None,
    }
}

/// Get all dependencies from a Rust project. Supports workspaces as well.
/// Returns a list of tuples with the dependency name and version.
pub fn get_cargo_dependencies(project: &crate::project::Project) -> Result<Vec<(String, String)>> {
    let mut dependencies = Vec::new();
    let cargo_path = project.root().join("Cargo.toml");
    
    // Check if Cargo.toml exists first
    if !cargo_path.exists() {
        tracing::error!("Cargo.toml not found at path: {:?}", cargo_path);
        // Return empty dependencies instead of error to allow projects without Cargo.toml
        return Ok(Vec::new());
    }
    
    // Read the Cargo.toml file with better error handling
    let cargo_content = match fs::read_to_string(&cargo_path) {
        Ok(content) => content,
        Err(e) => {
            tracing::error!("Failed to read Cargo.toml at {:?}: {}", cargo_path, e);
            if cfg!(windows) {
                tracing::error!("Windows path issue: Check if path contains special characters or spaces");
            }
            // Return empty dependencies instead of error
            return Ok(Vec::new());
        }
    };
    
    // Parse the TOML with better error handling
    let cargo_toml: Value = match toml::from_str(&cargo_content) {
        Ok(parsed) => parsed,
        Err(e) => {
            tracing::error!("Failed to parse Cargo.toml: {}", e);
            // Return empty dependencies instead of error
            return Ok(Vec::new());
        }
    };

    // Helper function to extract dependencies and versions
    fn extract_deps(table: &Value) -> Vec<(String, String)> {
        table
            .as_table()
            .map(|t| {
                t.iter()
                    .filter_map(|(name, val)| {
                        let version = match val {
                            Value::String(v) => Some(v.clone()),
                            Value::Table(t) => t.get("version")?.as_str()?.to_string().into(),
                            _ => None,
                        }?;
                        Some((name.clone(), version))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    // Parse workspace dependencies if they exist
    if let Some(workspace) = cargo_toml.get("workspace") {
        if let Some(workspace_deps) = workspace.get("dependencies") {
            dependencies.extend(extract_deps(workspace_deps));
        }
    }

    // Get workspace members
    let members = if let Some(workspace) = cargo_toml.get("workspace") {
        workspace
            .get("members")
            .and_then(|m| m.as_array())
            .map(|patterns| {
                patterns
                    .iter()
                    .filter_map(|p| p.as_str())
                    .flat_map(|pattern| {
                        let p = project.root().join(pattern).display().to_string();
                        match glob::glob(&p) {
                            Ok(paths) => paths.collect::<Vec<_>>(),
                            Err(e) => {
                                tracing::warn!("Error in glob pattern '{}': {}", p, e);
                                vec![Ok(PathBuf::from(p))]
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        // If not a workspace, treat as single package
        vec![Ok(project.root().to_path_buf())]
    };

    // Parse dependencies from each member
    for member_path in members {
        let Ok(member_path) = member_path else {
            tracing::error!("Error processing workspace member: {:?}", member_path);
            continue;
        };
        let member_cargo_path = member_path.join("Cargo.toml");
        if member_cargo_path.exists() {
            tracing::debug!("Processing member: {:?}", member_path);
            
            // Read the member's Cargo.toml with error handling
            let member_content = match fs::read_to_string(&member_cargo_path) {
                Ok(content) => content,
                Err(e) => {
                    tracing::error!("Failed to read member Cargo.toml at {:?}: {}", member_cargo_path, e);
                    continue;
                }
            };
            
            // Parse the member's TOML with error handling
            let member_toml: Value = match toml::from_str(&member_content) {
                Ok(parsed) => parsed,
                Err(e) => {
                    tracing::error!("Failed to parse member Cargo.toml at {:?}: {}", member_cargo_path, e);
                    continue;
                }
            };

            // Get dependencies from different sections
            if let Some(deps) = member_toml.get("dependencies") {
                dependencies.extend(extract_deps(deps));
            }
            if let Some(dev_deps) = member_toml.get("dev-dependencies") {
                dependencies.extend(extract_deps(dev_deps));
            }
            if let Some(target) = member_toml.get("target") {
                if let Some(target_table) = target.as_table() {
                    for target_cfg in target_table.values() {
                        if let Some(target_deps) = target_cfg.get("dependencies") {
                            dependencies.extend(extract_deps(target_deps));
                        }
                    }
                }
            }
        }
    }

    // Deduplicate dependencies (keep last occurrence)
    dependencies.sort_by(|a, b| a.0.cmp(&b.0));
    dependencies.dedup_by(|a, b| a.0 == b.0);
    
    tracing::debug!("Found {} dependencies", dependencies.len());
    Ok(dependencies)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rust_symbol() {
        assert_eq!(
            parse_rust_symbol("fn.Ok.html"),
            Some(RustSymbol::Function("Ok"))
        );
        assert_eq!(
            parse_rust_symbol("macro.ensure!.html"),
            Some(RustSymbol::Macro("ensure"))
        );
        assert_eq!(
            parse_rust_symbol("struct.Chain.html"),
            Some(RustSymbol::Struct("Chain"))
        );
        assert_eq!(
            parse_rust_symbol("trait.Context.html"),
            Some(RustSymbol::Trait("Context"))
        );
        assert_eq!(parse_rust_symbol("invalid"), None);
    }

    #[test]
    fn test_to_string() {
        assert_eq!(RustSymbol::Function("Ok").to_string(), "fn Ok");
        assert_eq!(RustSymbol::Macro("ensure").to_string(), "macro ensure!");
        assert_eq!(RustSymbol::Struct("Chain").to_string(), "struct Chain");
        assert_eq!(RustSymbol::Trait("Context").to_string(), "trait Context");
        assert_eq!(RustSymbol::Type("Result").to_string(), "type Result");
        assert_eq!(RustSymbol::Enum("Option").to_string(), "enum Option");
    }
}
