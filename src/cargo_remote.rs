use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json as json;
use tokio::process::Command;

use crate::project::Project;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum CargoMessage {
    CompilerArtifact(json::Value),
    BuildScriptExecuted(json::Value),
    CompilerMessage { message: CompilerMessage },
    BuildFinished { success: bool },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CompilerMessage {
    pub rendered: String,
    pub code: Option<json::Value>,
    pub level: String,
    pub spans: Vec<CompilerMessageSpan>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CompilerMessageSpan {
    pub column_start: usize,
    pub column_end: usize,
    pub file_name: String,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Clone, Debug)]
pub struct CargoRemote {
    repository: Project,
}

impl Default for CargoRemote {
    fn default() -> Self {
        // Create a dummy project for the default implementation
        // This will be replaced when properly initialized
        Self {
            repository: Project {
                root: std::path::PathBuf::new(),
                ignore_crates: Vec::new(),
            },
        }
    }
}

impl CargoRemote {
    pub fn new(repository: Project) -> Self {
        Self { repository }
    }

    async fn run_cargo_command(
        &self,
        args: &[&str],
        backtrace: bool,
    ) -> Result<(Vec<CargoMessage>, Vec<String>)> {
        let output = Command::new("cargo")
            .current_dir(self.repository.root())
            .args(args)
            .env("RUST_BACKTRACE", if backtrace { "full" } else { "0" })
            .output()
            .await?;

        let stdout = String::from_utf8(output.stdout)?;

        let mut messages = Vec::new();
        let mut test_messages = Vec::new();
        for line in stdout.lines().filter(|line| !line.is_empty()) {
            match json::from_str::<CargoMessage>(line) {
                Ok(message) => {
                    messages.push(message);
                }
                Err(_) => {
                    // Cargo test doesn't respect `message-format=json`
                    test_messages.push(line.to_string());
                }
            }
        }

        Ok((messages, test_messages))
    }

    pub async fn check(&self, only_errors: bool) -> Result<Vec<String>> {
        let (messages, _) = self
            .run_cargo_command(&["check", "--message-format=json"], false)
            .await?;
        Ok(messages
            .into_iter()
            .filter_map(|message| match message {
                CargoMessage::CompilerMessage { message } => {
                    if only_errors && message.level != "error" {
                        return None;
                    }
                    Some(message.rendered)
                }
                _ => None,
            })
            .collect::<Vec<_>>())
    }

    pub async fn test(&self, test_name: Option<String>, backtrace: bool) -> Result<Vec<String>> {
        let mut args = vec!["test", "--message-format=json"];
        if let Some(ref test_name) = test_name {
            args.push("--");
            args.push("--nocapture");
            args.push(test_name);
        }
        let (_, messages) = self.run_cargo_command(&args, backtrace).await?;
        Ok(messages)
    }
}
