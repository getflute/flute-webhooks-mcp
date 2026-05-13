use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;
use tokio::time::timeout;

use crate::error::FluteError;

#[async_trait]
pub trait CliRunner: Send + Sync {
    /// Run the upstream CLI with these argv tokens (the binary path is owned by the
    /// runner). On success returns the parsed JSON stdout. On failure returns a
    /// FluteError that already carries the structured envelope fields when available.
    async fn run(&self, args: &[String]) -> Result<Value, FluteError>;
}

/// Test double. Records every `run` call and returns canned responses in order.
pub struct MockRunner {
    pub calls: Mutex<Vec<Vec<String>>>,
    pub responses: Mutex<Vec<Result<Value, FluteError>>>,
}

impl MockRunner {
    pub fn new(responses: Vec<Result<Value, FluteError>>) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(responses),
        })
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl CliRunner for MockRunner {
    async fn run(&self, args: &[String]) -> Result<Value, FluteError> {
        self.calls.lock().unwrap().push(args.to_vec());
        let next = self.responses.lock().unwrap().drain(..1).next();
        match next {
            Some(r) => r,
            None => panic!("MockRunner: no canned response for call {:?}", args),
        }
    }
}

pub struct ProcessRunner {
    pub binary: PathBuf,
    pub timeout: Duration,
    pub debug: bool,
}

#[async_trait]
impl CliRunner for ProcessRunner {
    async fn run(&self, args: &[String]) -> Result<Value, FluteError> {
        let mut cmd = Command::new(&self.binary);
        cmd.args(args)
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let child = cmd
            .spawn()
            .map_err(|e| FluteError::Spawn(format!("{}: {}", self.binary.display(), e)))?;

        let output = match timeout(self.timeout, child.wait_with_output()).await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return Err(FluteError::Spawn(e.to_string())),
            Err(_) => {
                return Err(FluteError::Timeout {
                    secs: self.timeout.as_secs(),
                });
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if self.debug && !stderr.is_empty() {
            tracing::debug!(target: "flute_webhooks_mcp::runner", "flute-webhook stderr: {}", stderr);
        }

        let exit_code = output.status.code().unwrap_or(-1);

        if output.status.success() {
            // Empty stdout is legal for some commands (e.g. `endpoints delete`).
            if stdout.trim().is_empty() {
                return Ok(Value::Null);
            }
            serde_json::from_str(&stdout).map_err(|e| FluteError::Decode {
                message: format!("could not parse stdout as JSON: {e}"),
            })
        } else {
            Err(FluteError::from_envelope_stdout(exit_code, &stdout, &stderr))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[tokio::test]
    async fn mock_records_calls_and_replays_responses() {
        let mock = MockRunner::new(vec![Ok(json!({"data": []}))]);
        let out = mock.run(&["a".into(), "b".into()]).await.unwrap();
        assert_eq!(out, json!({"data": []}));
        assert_eq!(mock.calls(), vec![vec!["a".to_string(), "b".to_string()]]);
    }
}
