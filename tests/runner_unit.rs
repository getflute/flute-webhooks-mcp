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

#[tokio::test]
async fn api_envelope_failure_is_mapped() {
    let dir = TempDir::new().unwrap();
    let bin = write_script(
        &dir,
        "#!/bin/sh\nprintf '%s' '{\"kind\":\"api\",\"message\":\"bad\",\"status\":422,\"correlation_id\":\"x-1\"}'\nexit 1\n",
    );
    let runner = runner_for(bin, 5_000);
    let err = runner.run(&[]).await.unwrap_err();
    match err {
        FluteError::Api { status, message, correlation_id } => {
            assert_eq!(status, 422);
            assert_eq!(message, "bad");
            assert_eq!(correlation_id.as_deref(), Some("x-1"));
        }
        other => panic!("expected Api, got {other:?}"),
    }
}

#[tokio::test]
async fn auth_envelope_failure_is_mapped() {
    let dir = TempDir::new().unwrap();
    let bin = write_script(
        &dir,
        "#!/bin/sh\nprintf '%s' '{\"kind\":\"auth\",\"message\":\"no credentials for [uat]\"}'\nexit 1\n",
    );
    let runner = runner_for(bin, 5_000);
    let err = runner.run(&[]).await.unwrap_err();
    assert!(matches!(err, FluteError::Auth { message } if message.contains("no credentials")));
}

#[tokio::test]
async fn unparseable_failure_becomes_bad_output() {
    let dir = TempDir::new().unwrap();
    let bin = write_script(&dir, "#!/bin/sh\necho 'totally not json' >&1\nexit 7\n");
    let runner = runner_for(bin, 5_000);
    let err = runner.run(&[]).await.unwrap_err();
    match err {
        FluteError::BadOutput { exit_code, stdout, .. } => {
            assert_eq!(exit_code, 7);
            assert!(stdout.contains("totally not json"));
        }
        other => panic!("expected BadOutput, got {other:?}"),
    }
}

#[tokio::test]
async fn missing_binary_produces_spawn_error() {
    let runner = ProcessRunner {
        binary: PathBuf::from("/no/such/binary/anywhere"),
        timeout: Duration::from_secs(1),
        debug: false,
    };
    let err = runner.run(&[]).await.unwrap_err();
    assert!(matches!(err, FluteError::Spawn(_)));
}

#[tokio::test]
async fn timeout_kills_the_child() {
    let dir = TempDir::new().unwrap();
    let bin = write_script(&dir, "#!/bin/sh\nsleep 10\n");
    let runner = runner_for(bin, 100);
    let start = std::time::Instant::now();
    let err = runner.run(&[]).await.unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(2), "should not have waited for sleep");
    assert!(matches!(err, FluteError::Timeout { .. }));
}
