use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use flute_webhooks_mcp::error::FluteError;
use flute_webhooks_mcp::runner::{CliRunner, ProcessRunner};
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

fn write_script(dir: &TempDir, body: &str) -> PathBuf {
    let path = dir.path().join("flute-webhook");
    fs::write(&path, body).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

fn runner_for(binary: PathBuf, timeout_ms: u64) -> ProcessRunner {
    ProcessRunner {
        binary,
        timeout: Duration::from_millis(timeout_ms),
        debug: false,
    }
}

#[tokio::test]
async fn success_returns_parsed_json() {
    let dir = TempDir::new().unwrap();
    let bin = write_script(
        &dir,
        "#!/bin/sh\nprintf '%s' '{\"data\":[{\"id\":\"e1\"}]}'\n",
    );
    let runner = runner_for(bin, 5_000);
    let out = runner.run(&["--profile".into(), "uat".into()]).await.unwrap();
    assert_eq!(out, json!({"data":[{"id":"e1"}]}));
}

#[tokio::test]
async fn empty_stdout_on_success_returns_null() {
    let dir = TempDir::new().unwrap();
    let bin = write_script(&dir, "#!/bin/sh\nexit 0\n");
    let runner = runner_for(bin, 5_000);
    let out = runner.run(&[]).await.unwrap();
    assert_eq!(out, serde_json::Value::Null);
}
