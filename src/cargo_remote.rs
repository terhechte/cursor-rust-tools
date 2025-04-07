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
    CompilerMessage {
        message: CompilerMessage,
    },
    BuildFinished {
        success: bool,
    },
    #[serde(rename = "test-message")]
    TestMessage(TestMessage),
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TestMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub name: Option<String>,
    pub event: String,
    pub stdout: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CargoRemote {
    repository: Project,
}

impl CargoRemote {
    pub fn new(repository: Project) -> Self {
        Self { repository }
    }

    async fn run_cargo_command(&self, args: &[&str]) -> Result<Vec<CargoMessage>> {
        let output = Command::new("cargo")
            .current_dir(self.repository.root())
            .args(args)
            .output()
            .await?;

        let stdout = String::from_utf8(output.stdout)?;
        let messages: Vec<CargoMessage> = stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(json::from_str)
            .collect::<Result<_, _>>()?;

        Ok(messages)
    }

    #[allow(dead_code)]
    pub async fn build(&self) -> Result<Vec<CargoMessage>> {
        self.run_cargo_command(&["build", "--message-format=json"])
            .await
    }

    pub async fn check(&self) -> Result<Vec<CargoMessage>> {
        self.run_cargo_command(&["check", "--message-format=json"])
            .await
    }

    pub async fn test(&self, test_name: Option<String>) -> Result<Vec<CargoMessage>> {
        let mut args = vec!["test", "--message-format=json"];
        if let Some(ref test_name) = test_name {
            args.push("--");
            args.push(test_name);
        }
        self.run_cargo_command(&args).await
    }

    #[allow(dead_code)]
    pub async fn fmt(&self) -> Result<()> {
        let output = Command::new("cargo")
            .current_dir(self.repository.root())
            .args(["fmt"])
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!("cargo fmt failed");
        }

        Ok(())
    }
}
