use anyhow::Result;
use serde::Deserialize;
use serde_json as json;
use std::path::PathBuf;
use std::process::Command;

use crate::project::Repository;

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CompilerMessage {
    pub rendered: String,
    pub code: Option<json::Value>,
    pub level: String,
    pub spans: Vec<CompilerMessageSpan>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CompilerMessageSpan {
    pub column_start: usize,
    pub column_end: usize,
    pub file_name: String,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TestMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub name: Option<String>,
    pub event: String,
    pub stdout: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CargoRemote {
    repository: Repository,
}

impl CargoRemote {
    pub fn new(repository: Repository) -> Self {
        Self { repository }
    }

    fn run_cargo_command(&self, args: &[&str]) -> Result<Vec<CargoMessage>> {
        let output = Command::new("cargo")
            .current_dir(self.repository.path())
            .args(args)
            .output()?;

        let stdout = String::from_utf8(output.stdout)?;
        let messages: Vec<CargoMessage> = stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| json::from_str(line))
            .collect::<Result<_, _>>()?;

        Ok(messages)
    }

    pub fn build(&self) -> Result<Vec<CargoMessage>> {
        self.run_cargo_command(&["build", "--message-format=json"])
    }

    pub fn check(&self) -> Result<Vec<CargoMessage>> {
        self.run_cargo_command(&["check", "--message-format=json"])
    }

    pub fn test(&self) -> Result<Vec<CargoMessage>> {
        self.run_cargo_command(&["test", "--message-format=json"])
    }

    pub fn fmt(&self) -> Result<()> {
        let output = Command::new("cargo")
            .current_dir(self.repository.path())
            .args(["fmt"])
            .output()?;

        if !output.status.success() {
            anyhow::bail!("cargo fmt failed");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cargo_remote() {
        let repository = Repository::new(PathBuf::from("assets/zoxide-main")).unwrap();
        let cargo_remote = CargoRemote::new(repository);
        let messages = cargo_remote.check().unwrap();
        println!("{:?}", messages);
    }
}
