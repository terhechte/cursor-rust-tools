use crate::project::Project;
use anyhow::Result;
use std::process::Command;

pub fn generate_docs(project: &Project) -> Result<()> {
    // Create the output directory path
    // let output_dir = repo.cache_dir().join("gen_docs");
    // let output_dir = PathBuf::from("target/doc");

    // Run cargo doc with custom output directory
    let status = Command::new("cargo")
        .current_dir(project.root())
        .args([
            "doc",
            "--target-dir", // Specify custom target directory
            project.cache_folder(),
        ])
        .status()?;

    if !status.success() {
        return Err(anyhow::anyhow!("Failed to generate documentation"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_generate_docs() {
        // let (repository, guard) = crate::test_utils::test_repository();
        let project = Project::new(PathBuf::from("assets/zoxide-main")).unwrap();
        generate_docs(&project).unwrap();
        // guard.keep();
    }
}
