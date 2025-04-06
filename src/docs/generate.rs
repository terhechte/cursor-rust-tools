use crate::project::Project;
use anyhow::Result;
use std::process::Command;

pub fn generate_docs(project: &Project) -> Result<()> {
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
