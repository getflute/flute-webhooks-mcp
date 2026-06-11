# flute-webhooks-mcp Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a stdio MCP server in Rust that exposes the agent-safe subset of the `flute-webhook` CLI as MCP tools.

**Architecture:** Single Rust binary using `rmcp` 1.7 (official MCP SDK) for the stdio transport and tool router. A `CliRunner` trait abstracts subprocess execution; the real impl uses `tokio::process::Command` to spawn `flute-webhook --output json …` once per tool call, and a `MockRunner` is used for unit tests. The upstream CLI's structured error envelope (`{kind, message, status?, correlation_id?}`) is preserved end-to-end as `FluteError` and mapped to MCP `isError: true` results with the structured fields intact.

**Tech Stack:** Rust edition 2024, `rmcp` 1.7, `tokio`, `serde`/`serde_json`, `schemars`, `thiserror`, `tracing`, `which`, `clap`.

**Spec:** `docs/superpowers/specs/2026-05-13-flute-webhooks-mcp-design.md`

**Pre-flight (engineer reads once before starting):**
- The upstream binary is named `flute-webhook` (singular) and lives in a private repo; the contract we depend on is documented in its `AGENTS.md`.
- Every non-TUI subcommand accepts `--output json`. On success it prints JSON to stdout, exit 0. On failure it prints `{"kind":..., "message":..., "status"?:..., "correlation_id"?:...}` to stdout and exits non-zero. stderr is human-readable tracing.
- Global flags work *before* the subcommand: `--profile <sandbox|production> --output json …`.
- `endpoints delete` requires `--yes` and emits empty stdout on success; we synthesize a result.
- rmcp 1.7 macros: `#[tool(description = "…")]` on async methods, `#[tool_router]` on the impl block, `#[tool_handler]` on `impl ServerHandler`. Inputs use `Parameters<T>` where `T: serde::Deserialize + schemars::JsonSchema`.

---

## File structure

| Path | Purpose |
|---|---|
| `Cargo.toml` | Crate manifest with locked dep versions. |
| `.gitignore` | Ignore `target/`. |
| `src/main.rs` | Binary entry: clap, tracing, build config + runner + server, run on stdio. |
| `src/lib.rs` | `pub mod` declarations; re-exports for tests. |
| `src/error.rs` | `FluteError` enum + envelope deserializer + conversion to `rmcp` errors. |
| `src/config.rs` | `Config { profile, binary, timeout }` + env-var loader. |
| `src/runner.rs` | `CliRunner` trait, `ProcessRunner`, `MockRunner`. |
| `src/server.rs` | `FluteServer` struct, `#[tool_router]`/`#[tool_handler]` impls, all tools. |
| `tests/runner_unit.rs` | `ProcessRunner` against fake shell scripts in a tempdir. |
| `tests/tools_with_mock.rs` | Each tool against `MockRunner` — argv shape + error pass-through. |
| `tests/e2e_stdio.rs` | Spawn our binary + fake `flute-webhook`, exchange real MCP frames. |
| `readme.md` | Operator install + Claude Desktop snippet + env-var reference. |

**Note:** all tool methods live on a single `FluteServer` struct in `server.rs` because rmcp's `#[tool_router]` operates on one impl block. Tool *helpers* (argv building) can live in private functions inside `server.rs` to keep the file focused; if it grows past ~600 lines, split helpers into a `src/argv.rs` module in a follow-up.

---

## Task 1: Cargo.toml + .gitignore + lib.rs skeleton

**Files:**
- Modify: `Cargo.toml`
- Create: `.gitignore`
- Create: `src/lib.rs`
- Modify: `src/main.rs` (currently `Hello, world!`)

- [ ] **Step 1: Replace `Cargo.toml`**

```toml
[package]
name = "flute-webhooks-mcp"
version = "0.1.0"
edition = "2024"
description = "MCP server that drives the flute-webhook CLI"
license = "MIT"

[[bin]]
name = "flute-webhooks-mcp"
path = "src/main.rs"

[lib]
name = "flute_webhooks_mcp"
path = "src/lib.rs"

[dependencies]
rmcp = { version = "1.7", features = ["server", "transport-io", "macros", "schemars"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "process", "time", "signal", "io-util"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
which = "8"
clap = { version = "4", features = ["derive", "env"] }
async-trait = "0.1"

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "process", "time", "test-util"] }
pretty_assertions = "1"
tempfile = "3"
assert_cmd = "2"
serde_json = "1"
```

- [ ] **Step 2: Create `.gitignore`**

```
/target
```

- [ ] **Step 3: Replace `src/main.rs`**

```rust
fn main() {
    println!("flute-webhooks-mcp — wire-up pending");
}
```

- [ ] **Step 4: Create `src/lib.rs`**

```rust
pub mod config;
pub mod error;
pub mod runner;
pub mod server;
```

- [ ] **Step 5: Create empty module stubs so the crate compiles**

Create `src/config.rs`, `src/error.rs`, `src/runner.rs`, `src/server.rs` — each containing only:

```rust
// stub — implemented in a later task
```

- [ ] **Step 6: Verify the crate compiles**

Run: `cargo check`
Expected: clean compile, only "unused" warnings.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore src/main.rs src/lib.rs src/config.rs src/error.rs src/runner.rs src/server.rs
git commit -m "scaffold: add manifest, gitignore, and module stubs"
```

---

## Task 2: `FluteError` + envelope deserializer

**Files:**
- Modify: `src/error.rs`

The envelope from the upstream CLI: `{"kind":"api"|"transport"|"auth"|"decode"|"client", "message":"...", "status":422, "correlation_id":"abc"}`. Only `kind`+`message` are guaranteed; `status` is present only for `kind:"api"`; `correlation_id` is optional even within `api`.

- [ ] **Step 1: Write the failing test**

Replace `src/error.rs` with:

```rust
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FluteError {
    #[error("api error (status {status}): {message}")]
    Api {
        status: u16,
        message: String,
        correlation_id: Option<String>,
    },
    #[error("transport error: {message}")]
    Transport { message: String },
    #[error("auth error: {message} — run `flute-webhook auth login`")]
    Auth { message: String },
    #[error("decode error: {message}")]
    Decode { message: String },
    #[error("cli usage error: {message}")]
    Client { message: String },
    #[error("could not spawn flute-webhook: {0}")]
    Spawn(String),
    #[error("flute-webhook timed out after {secs}s")]
    Timeout { secs: u64 },
    #[error("flute-webhook produced unparseable output (exit {exit_code})")]
    BadOutput {
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
}

#[derive(Debug, Deserialize)]
struct Envelope {
    kind: String,
    message: String,
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    correlation_id: Option<String>,
}

impl FluteError {
    pub fn from_envelope_stdout(
        exit_code: i32,
        stdout: &str,
        stderr: &str,
    ) -> FluteError {
        let parsed: Result<Envelope, _> = serde_json::from_str(stdout.trim());
        let Ok(env) = parsed else {
            return FluteError::BadOutput {
                exit_code,
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
            };
        };
        match env.kind.as_str() {
            "api" => FluteError::Api {
                status: env.status.unwrap_or(0),
                message: env.message,
                correlation_id: env.correlation_id,
            },
            "transport" => FluteError::Transport { message: env.message },
            "auth" => FluteError::Auth { message: env.message },
            "decode" => FluteError::Decode { message: env.message },
            "client" => FluteError::Client { message: env.message },
            _ => FluteError::BadOutput {
                exit_code,
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_api_envelope_with_all_fields() {
        let body = r#"{"kind":"api","message":"validation failed","status":422,"correlation_id":"abc-123"}"#;
        match FluteError::from_envelope_stdout(1, body, "") {
            FluteError::Api { status, message, correlation_id } => {
                assert_eq!(status, 422);
                assert_eq!(message, "validation failed");
                assert_eq!(correlation_id.as_deref(), Some("abc-123"));
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn parses_auth_envelope_without_status() {
        let body = r#"{"kind":"auth","message":"no credentials"}"#;
        assert!(matches!(
            FluteError::from_envelope_stdout(1, body, ""),
            FluteError::Auth { message } if message == "no credentials"
        ));
    }

    #[test]
    fn unparseable_stdout_becomes_bad_output() {
        let result = FluteError::from_envelope_stdout(2, "not json", "stderr text");
        match result {
            FluteError::BadOutput { exit_code, stdout, stderr } => {
                assert_eq!(exit_code, 2);
                assert_eq!(stdout, "not json");
                assert_eq!(stderr, "stderr text");
            }
            other => panic!("expected BadOutput, got {other:?}"),
        }
    }

    #[test]
    fn unknown_kind_becomes_bad_output() {
        let body = r#"{"kind":"martian","message":"x"}"#;
        assert!(matches!(
            FluteError::from_envelope_stdout(1, body, ""),
            FluteError::BadOutput { .. }
        ));
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib error::tests`
Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/error.rs
git commit -m "feat: FluteError envelope deserializer with tests"
```

---

## Task 3: `Config` + env loading

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write the failing test**

Replace `src/config.rs` with:

```rust
use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Sandbox,
    Production,
}

impl Profile {
    pub fn as_cli_str(self) -> &'static str {
        match self {
            Profile::Sandbox => "sandbox",
            Profile::Production => "production",
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid FLUTE_PROFILE value `{0}` (expected `sandbox` or `production`)")]
    InvalidProfile(String),
    #[error("invalid FLUTE_MCP_TIMEOUT_SECS value `{0}` (expected positive integer)")]
    InvalidTimeout(String),
    #[error("could not locate `flute-webhook` on PATH and FLUTE_WEBHOOK_BIN is unset")]
    BinaryNotFound,
    #[error("FLUTE_WEBHOOK_BIN=`{0}` does not exist or is not executable")]
    BinaryUnusable(String),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub profile: Profile,
    pub binary: PathBuf,
    pub timeout: Duration,
    pub debug: bool,
}

impl Config {
    /// Build a Config from a closure that returns env vars (so tests can inject).
    pub fn from_env<F>(getenv: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let profile = match getenv("FLUTE_PROFILE").as_deref() {
            None | Some("") | Some("sandbox") => Profile::Sandbox,
            Some("production") | Some("prod") => Profile::Production,
            Some(other) => return Err(ConfigError::InvalidProfile(other.to_string())),
        };

        let timeout = match getenv("FLUTE_MCP_TIMEOUT_SECS") {
            None => Duration::from_secs(30),
            Some(s) => {
                let n: u64 = s
                    .parse()
                    .map_err(|_| ConfigError::InvalidTimeout(s.clone()))?;
                if n == 0 {
                    return Err(ConfigError::InvalidTimeout(s));
                }
                Duration::from_secs(n)
            }
        };

        let binary = match getenv("FLUTE_WEBHOOK_BIN") {
            Some(p) if !p.is_empty() => {
                let path = PathBuf::from(&p);
                if !path.is_file() {
                    return Err(ConfigError::BinaryUnusable(p));
                }
                path
            }
            _ => which::which("flute-webhook").map_err(|_| ConfigError::BinaryNotFound)?,
        };

        let debug = matches!(getenv("FLUTE_MCP_DEBUG").as_deref(), Some(v) if !v.is_empty());

        Ok(Self { profile, binary, timeout, debug })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn make_env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + '_ {
        let map: HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| map.get(k).cloned()
    }

    fn fake_binary(dir: &TempDir, name: &str) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn defaults_to_sandbox_and_30s() {
        let dir = TempDir::new().unwrap();
        let bin = fake_binary(&dir, "flute-webhook");
        let env = make_env(&[("FLUTE_WEBHOOK_BIN", bin.to_str().unwrap())]);
        let cfg = Config::from_env(env).unwrap();
        assert_eq!(cfg.profile, Profile::Sandbox);
        assert_eq!(cfg.timeout, Duration::from_secs(30));
        assert!(!cfg.debug);
    }

    #[test]
    fn accepts_production_and_prod_alias() {
        let dir = TempDir::new().unwrap();
        let bin = fake_binary(&dir, "flute-webhook");
        for value in ["production", "prod"] {
            let env = make_env(&[
                ("FLUTE_WEBHOOK_BIN", bin.to_str().unwrap()),
                ("FLUTE_PROFILE", value),
            ]);
            let cfg = Config::from_env(env).unwrap();
            assert_eq!(cfg.profile, Profile::Production);
        }
    }

    #[test]
    fn rejects_unknown_profile() {
        let dir = TempDir::new().unwrap();
        let bin = fake_binary(&dir, "flute-webhook");
        let env = make_env(&[
            ("FLUTE_WEBHOOK_BIN", bin.to_str().unwrap()),
            ("FLUTE_PROFILE", "staging"),
        ]);
        assert!(matches!(
            Config::from_env(env),
            Err(ConfigError::InvalidProfile(s)) if s == "staging"
        ));
    }

    #[test]
    fn missing_binary_errors() {
        // FLUTE_WEBHOOK_BIN unset AND we set PATH to a dir with no flute-webhook.
        // `which::which` consults the real PATH; for a deterministic test we use
        // an explicit path that doesn't exist.
        let env = make_env(&[("FLUTE_WEBHOOK_BIN", "/nope/does/not/exist")]);
        assert!(matches!(
            Config::from_env(env),
            Err(ConfigError::BinaryUnusable(_))
        ));
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib config::tests`
Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat: Config with env-driven profile/binary/timeout"
```

---

## Task 4: `CliRunner` trait + `MockRunner`

**Files:**
- Modify: `src/runner.rs`

- [ ] **Step 1: Add the trait, the MockRunner, and its tests**

Replace `src/runner.rs` with:

```rust
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;

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
```

- [ ] **Step 2: Run the test**

Run: `cargo test --lib runner::tests`
Expected: 1 test passes.

- [ ] **Step 3: Commit**

```bash
git add src/runner.rs
git commit -m "feat: CliRunner trait and MockRunner test double"
```

---

## Task 5: `ProcessRunner` — happy path with a fake binary

**Files:**
- Modify: `src/runner.rs`
- Create: `tests/runner_unit.rs`

We use a tempdir-hosted POSIX shell script as a stand-in for the real `flute-webhook` binary. Each test writes a fresh script that prints canned stdout and exits with a chosen code.

- [ ] **Step 1: Append `ProcessRunner` to `src/runner.rs`**

Add to the bottom of `src/runner.rs` (before the `#[cfg(test)]` block):

```rust
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

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
```

- [ ] **Step 2: Create `tests/runner_unit.rs`**

```rust
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
    let out = runner.run(&["--profile".into(), "sandbox".into()]).await.unwrap();
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
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --test runner_unit`
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/runner.rs tests/runner_unit.rs
git commit -m "feat: ProcessRunner happy path"
```

---

## Task 6: `ProcessRunner` — error paths

**Files:**
- Modify: `tests/runner_unit.rs`

- [ ] **Step 1: Append error-path tests**

Append to `tests/runner_unit.rs`:

```rust
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
        "#!/bin/sh\nprintf '%s' '{\"kind\":\"auth\",\"message\":\"no credentials for [sandbox]\"}'\nexit 1\n",
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
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --test runner_unit`
Expected: 6 tests pass total.

- [ ] **Step 3: Commit**

```bash
git add tests/runner_unit.rs
git commit -m "test: ProcessRunner envelope/auth/badoutput/spawn paths"
```

---

## Task 7: `ProcessRunner` — timeout path

**Files:**
- Modify: `tests/runner_unit.rs`

- [ ] **Step 1: Append timeout test**

Append to `tests/runner_unit.rs`:

```rust
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
```

- [ ] **Step 2: Run the test**

Run: `cargo test --test runner_unit timeout_kills_the_child`
Expected: 1 test passes; total suite 7 passes.

- [ ] **Step 3: Commit**

```bash
git add tests/runner_unit.rs
git commit -m "test: ProcessRunner timeout kills the child"
```

---

## Task 8: argv helper + first tool (`endpoints_list`)

**Files:**
- Modify: `src/server.rs`
- Create: `tests/tools_with_mock.rs`

This task introduces the rmcp glue *and* the first tool so the pattern is real. Later tasks add tools by repeating it.

- [ ] **Step 1: Replace `src/server.rs`**

```rust
use std::sync::Arc;

use rmcp::{
    handler::server::router::tool::ToolRouter,
    handler::server::tool::Parameters,
    model::{CallToolResult, Content, ErrorData as McpError, ServerInfo},
    schemars,
    tool, tool_handler, tool_router,
    ServerHandler,
};
use serde::Deserialize;
use serde_json::Value;

use crate::config::{Config, Profile};
use crate::error::FluteError;
use crate::runner::CliRunner;

#[derive(Clone)]
pub struct FluteServer {
    config: Arc<Config>,
    runner: Arc<dyn CliRunner>,
    tool_router: ToolRouter<Self>,
}

impl FluteServer {
    pub fn new(config: Arc<Config>, runner: Arc<dyn CliRunner>) -> Self {
        Self {
            config,
            runner,
            tool_router: Self::tool_router(),
        }
    }

    /// Build an argv that always starts with the pinned global flags.
    fn base_args(&self) -> Vec<String> {
        vec![
            "--profile".into(),
            self.config.profile.as_cli_str().into(),
            "--output".into(),
            "json".into(),
        ]
    }

    async fn run_cli(&self, args: Vec<String>) -> Result<Value, FluteError> {
        self.runner.run(&args).await
    }
}

/// Convert a FluteError into a structured MCP error result (isError=true, JSON content).
fn flute_err_to_result(err: FluteError) -> CallToolResult {
    let payload = match &err {
        FluteError::Api { status, message, correlation_id } => serde_json::json!({
            "kind": "api",
            "status": status,
            "message": message,
            "correlation_id": correlation_id,
        }),
        FluteError::Transport { message } => serde_json::json!({
            "kind": "transport", "message": message,
        }),
        FluteError::Auth { message } => serde_json::json!({
            "kind": "auth",
            "message": format!("{message} — run `flute-webhook auth login`"),
        }),
        FluteError::Decode { message } => serde_json::json!({
            "kind": "decode", "message": message,
        }),
        FluteError::Client { message } => serde_json::json!({
            "kind": "client", "message": message,
        }),
        FluteError::Spawn(msg) => serde_json::json!({
            "kind": "spawn",
            "message": format!("could not spawn flute-webhook — set FLUTE_WEBHOOK_BIN or install the CLI ({msg})"),
        }),
        FluteError::Timeout { secs } => serde_json::json!({
            "kind": "timeout", "message": format!("flute-webhook timed out after {secs}s"),
        }),
        FluteError::BadOutput { exit_code, stdout, stderr } => {
            let stderr_trunc = if stderr.len() > 4096 { &stderr[..4096] } else { stderr.as_str() };
            serde_json::json!({
                "kind": "bad_output",
                "exit_code": exit_code,
                "stdout": stdout,
                "stderr": stderr_trunc,
            })
        }
    };
    CallToolResult::error(vec![Content::json(payload).unwrap()])
}

fn value_to_result(value: Value) -> CallToolResult {
    CallToolResult::success(vec![Content::json(value).unwrap()])
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct Empty {}

#[tool_router]
impl FluteServer {
    #[tool(
        description = "List all Flute webhook endpoints for the active profile. Pure read; safe to retry."
    )]
    pub async fn endpoints_list(
        &self,
        _params: Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend(["webhooks".into(), "endpoints".into(), "list".into()]);
        match self.run_cli(args).await {
            Ok(value) => Ok(value_to_result(value)),
            Err(e) => Ok(flute_err_to_result(e)),
        }
    }
}

#[tool_handler]
impl ServerHandler for FluteServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Drives the `flute-webhook` CLI. The active profile is pinned at server start; \
                 launch one instance per environment (sandbox vs production). Credentials are read \
                 from the OS keychain via `flute-webhook auth login` — run that first."
                    .into(),
            ),
            ..ServerInfo::default()
        }
    }
}
```

> **rmcp 1.7 note for the engineer:** if the exact import paths above don't match the version on crates.io, consult `cargo doc --open -p rmcp` and adjust. The pattern is canonical (`#[tool]` on async methods, `#[tool_router]` on the impl, `#[tool_handler]` on `impl ServerHandler`); only the module paths may shift between minor versions.

- [ ] **Step 2: Create `tests/tools_with_mock.rs`**

```rust
use std::sync::Arc;

use flute_webhooks_mcp::config::{Config, Profile};
use flute_webhooks_mcp::runner::MockRunner;
use flute_webhooks_mcp::server::FluteServer;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

fn cfg() -> Arc<Config> {
    Arc::new(Config {
        profile: Profile::Sandbox,
        binary: PathBuf::from("/dev/null"),  // ignored — MockRunner doesn't spawn
        timeout: Duration::from_secs(5),
        debug: false,
    })
}

#[tokio::test]
async fn endpoints_list_argv_matches_agents_md_spec() {
    let mock = MockRunner::new(vec![Ok(json!({"data": []}))]);
    let server = FluteServer::new(cfg(), mock.clone());

    use flute_webhooks_mcp::server::Empty;
    use rmcp::handler::server::tool::Parameters;
    let result = server.endpoints_list(Parameters(Empty {})).await.unwrap();

    assert_eq!(
        mock.calls(),
        vec![vec![
            "--profile".to_string(),
            "sandbox".into(),
            "--output".into(),
            "json".into(),
            "webhooks".into(),
            "endpoints".into(),
            "list".into(),
        ]]
    );
    // success result should not be flagged as an error
    assert_eq!(result.is_error.unwrap_or(false), false);
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test`
Expected: all suites green.

- [ ] **Step 4: Commit**

```bash
git add src/server.rs tests/tools_with_mock.rs
git commit -m "feat: FluteServer scaffold + endpoints_list tool"
```

---

## Task 9: Remaining endpoint tools (`get`, `create`, `update`, `delete`, `ping`)

**Files:**
- Modify: `src/server.rs`
- Modify: `tests/tools_with_mock.rs`

- [ ] **Step 1: Add input structs and tool methods**

In `src/server.rs`, add these structs **above** the `#[tool_router] impl FluteServer` block:

```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EndpointId {
    /// The webhook endpoint id (from `endpoints_list`).
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EndpointCreate {
    /// Destination URL for the webhook (must be https).
    pub url: String,
    /// Event-type names to subscribe to (see `event_types_list`).
    pub events: Vec<String>,
    /// Human-readable name; optional.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EndpointUpdate {
    pub id: String,
    #[serde(default)] pub url: Option<String>,
    #[serde(default)] pub events: Option<Vec<String>>,
    #[serde(default)] pub name: Option<String>,
    /// One of "active" | "inactive".
    #[serde(default)] pub status: Option<String>,
}
```

Add these methods **inside** the existing `#[tool_router] impl FluteServer { … }` block, after `endpoints_list`:

```rust
    #[tool(description = "Get a single webhook endpoint by id. Safe to retry.")]
    pub async fn endpoints_get(
        &self,
        Parameters(p): Parameters<EndpointId>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend([
            "webhooks".into(), "endpoints".into(), "get".into(), p.id,
        ]);
        Ok(match self.run_cli(args).await {
            Ok(v) => value_to_result(v),
            Err(e) => flute_err_to_result(e),
        })
    }

    #[tool(description = "Create a new webhook endpoint. NOT idempotent — duplicates create a second endpoint. Response includes a one-shot `secret` you must store; the API never returns it again.")]
    pub async fn endpoints_create(
        &self,
        Parameters(p): Parameters<EndpointCreate>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend([
            "webhooks".into(), "endpoints".into(), "create".into(),
            "--url".into(), p.url,
            "--events".into(), p.events.join(","),
        ]);
        if let Some(name) = p.name {
            args.extend(["--name".into(), name]);
        }
        Ok(match self.run_cli(args).await {
            Ok(v) => value_to_result(v),
            Err(e) => flute_err_to_result(e),
        })
    }

    #[tool(description = "Update an existing webhook endpoint (full-state PUT — safe to retry).")]
    pub async fn endpoints_update(
        &self,
        Parameters(p): Parameters<EndpointUpdate>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend([
            "webhooks".into(), "endpoints".into(), "update".into(), p.id,
        ]);
        if let Some(url) = p.url { args.extend(["--url".into(), url]); }
        if let Some(events) = p.events { args.extend(["--events".into(), events.join(",")]); }
        if let Some(name) = p.name { args.extend(["--name".into(), name]); }
        if let Some(status) = p.status { args.extend(["--status".into(), status]); }
        Ok(match self.run_cli(args).await {
            Ok(v) => value_to_result(v),
            Err(e) => flute_err_to_result(e),
        })
    }

    #[tool(description = "Delete a webhook endpoint by id. Second call returns 404; treat as idempotent.")]
    pub async fn endpoints_delete(
        &self,
        Parameters(p): Parameters<EndpointId>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend([
            "webhooks".into(), "endpoints".into(), "delete".into(), p.id.clone(), "--yes".into(),
        ]);
        Ok(match self.run_cli(args).await {
            Ok(_) => value_to_result(serde_json::json!({"deleted": true, "id": p.id})),
            Err(e) => flute_err_to_result(e),
        })
    }

    #[tool(description = "Send a test ping to a webhook endpoint. One-shot HTTP test, safe to retry.")]
    pub async fn endpoints_ping(
        &self,
        Parameters(p): Parameters<EndpointId>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend([
            "webhooks".into(), "endpoints".into(), "ping".into(), p.id,
        ]);
        Ok(match self.run_cli(args).await {
            Ok(v) => value_to_result(v),
            Err(e) => flute_err_to_result(e),
        })
    }
```

- [ ] **Step 2: Add tests for each endpoint tool**

Append to `tests/tools_with_mock.rs`:

```rust
use flute_webhooks_mcp::server::{EndpointCreate, EndpointId, EndpointUpdate, Empty};
use rmcp::handler::server::tool::Parameters;

#[tokio::test]
async fn endpoints_get_argv() {
    let mock = MockRunner::new(vec![Ok(json!({"id":"e1"}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server.endpoints_get(Parameters(EndpointId { id: "e1".into() })).await.unwrap();
    assert_eq!(mock.calls()[0], vec![
        "--profile", "sandbox", "--output", "json",
        "webhooks", "endpoints", "get", "e1",
    ]);
}

#[tokio::test]
async fn endpoints_create_argv_with_name() {
    let mock = MockRunner::new(vec![Ok(json!({"id":"e1","secret":"shh"}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server.endpoints_create(Parameters(EndpointCreate {
        url: "https://x.example/hook".into(),
        events: vec!["transaction.card.captured".into(), "refund.completed".into()],
        name: Some("My Hook".into()),
    })).await.unwrap();
    assert_eq!(mock.calls()[0], vec![
        "--profile", "sandbox", "--output", "json",
        "webhooks", "endpoints", "create",
        "--url", "https://x.example/hook",
        "--events", "transaction.card.captured,refund.completed",
        "--name", "My Hook",
    ]);
}

#[tokio::test]
async fn endpoints_update_only_includes_set_fields() {
    let mock = MockRunner::new(vec![Ok(json!({"id":"e1"}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server.endpoints_update(Parameters(EndpointUpdate {
        id: "e1".into(),
        url: None,
        events: None,
        name: None,
        status: Some("inactive".into()),
    })).await.unwrap();
    assert_eq!(mock.calls()[0], vec![
        "--profile", "sandbox", "--output", "json",
        "webhooks", "endpoints", "update", "e1",
        "--status", "inactive",
    ]);
}

#[tokio::test]
async fn endpoints_delete_synthesizes_result() {
    let mock = MockRunner::new(vec![Ok(serde_json::Value::Null)]);
    let server = FluteServer::new(cfg(), mock.clone());
    let result = server.endpoints_delete(Parameters(EndpointId { id: "e1".into() })).await.unwrap();
    assert_eq!(mock.calls()[0], vec![
        "--profile", "sandbox", "--output", "json",
        "webhooks", "endpoints", "delete", "e1", "--yes",
    ]);
    // Result content should contain `{"deleted": true, "id": "e1"}` — verify via the
    // public `content` field on CallToolResult.
    let content = result.content.expect("expected content");
    let first = content.first().expect("expected at least one content item");
    let json = first.as_text().expect("expected text content").text.as_str();
    assert!(json.contains("\"deleted\""), "missing deleted: {json}");
    assert!(json.contains("\"e1\""), "missing id: {json}");
}

#[tokio::test]
async fn auth_error_pass_through_is_isError() {
    use flute_webhooks_mcp::error::FluteError;
    let mock = MockRunner::new(vec![Err(FluteError::Auth {
        message: "no credentials for [sandbox]".into(),
    })]);
    let server = FluteServer::new(cfg(), mock.clone());
    let result = server.endpoints_list(Parameters(Empty {})).await.unwrap();
    assert_eq!(result.is_error.unwrap_or(false), true);
    let content = result.content.expect("expected content");
    let first = content.first().expect("expected at least one content item");
    let json = first.as_text().expect("expected text content").text.as_str();
    assert!(json.contains("\"auth\""), "missing kind=auth: {json}");
    assert!(json.contains("auth login"), "missing remediation hint: {json}");
}
```

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: green.

- [ ] **Step 4: Commit**

```bash
git add src/server.rs tests/tools_with_mock.rs
git commit -m "feat: endpoints_{get,create,update,delete,ping} tools"
```

---

## Task 10: `event_types_list`

**Files:**
- Modify: `src/server.rs`
- Modify: `tests/tools_with_mock.rs`

- [ ] **Step 1: Add the tool method**

Inside the `#[tool_router] impl FluteServer { … }` block:

```rust
    #[tool(description = "List the catalog of subscribable Flute event types. Safe to retry.")]
    pub async fn event_types_list(
        &self,
        _params: Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend(["webhooks".into(), "event-types".into(), "list".into()]);
        Ok(match self.run_cli(args).await {
            Ok(v) => value_to_result(v),
            Err(e) => flute_err_to_result(e),
        })
    }
```

- [ ] **Step 2: Append the test**

Append to `tests/tools_with_mock.rs`:

```rust
#[tokio::test]
async fn event_types_list_argv() {
    let mock = MockRunner::new(vec![Ok(json!({"data": []}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server.event_types_list(Parameters(Empty {})).await.unwrap();
    assert_eq!(mock.calls()[0], vec![
        "--profile", "sandbox", "--output", "json",
        "webhooks", "event-types", "list",
    ]);
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: green.

- [ ] **Step 4: Commit**

```bash
git add src/server.rs tests/tools_with_mock.rs
git commit -m "feat: event_types_list tool"
```

---

## Task 11: deliveries tools

**Files:**
- Modify: `src/server.rs`
- Modify: `tests/tools_with_mock.rs`

- [ ] **Step 1: Add input structs**

Above the `#[tool_router]` block:

```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeliveriesList {
    #[serde(default)] pub endpoint_id: Option<String>,
    /// "success" or "failed".
    #[serde(default)] pub status: Option<String>,
    /// 1..=200 (server default 50).
    #[serde(default)] pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeliveryId {
    pub id: String,
}
```

- [ ] **Step 2: Add the three tool methods**

Inside the `#[tool_router]` impl block:

```rust
    #[tool(description = "List delivery log entries, optionally filtered by endpoint, status, and limit. Safe to retry.")]
    pub async fn deliveries_list(
        &self,
        Parameters(p): Parameters<DeliveriesList>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend(["webhooks".into(), "deliveries".into(), "list".into()]);
        if let Some(eid) = p.endpoint_id { args.extend(["--endpoint-id".into(), eid]); }
        if let Some(s) = p.status { args.extend(["--status".into(), s]); }
        if let Some(n) = p.limit { args.extend(["--limit".into(), n.to_string()]); }
        Ok(match self.run_cli(args).await {
            Ok(v) => value_to_result(v),
            Err(e) => flute_err_to_result(e),
        })
    }

    #[tool(description = "Get a single delivery log with full request and response bodies. Safe to retry.")]
    pub async fn deliveries_get(
        &self,
        Parameters(p): Parameters<DeliveryId>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend(["webhooks".into(), "deliveries".into(), "get".into(), p.id]);
        Ok(match self.run_cli(args).await {
            Ok(v) => value_to_result(v),
            Err(e) => flute_err_to_result(e),
        })
    }

    #[tool(description = "Re-schedule a failed delivery. NOT idempotent — each call schedules an additional retry. Check `deliveries_get` before retrying again.")]
    pub async fn deliveries_retry(
        &self,
        Parameters(p): Parameters<DeliveryId>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend(["webhooks".into(), "deliveries".into(), "retry".into(), p.id]);
        Ok(match self.run_cli(args).await {
            Ok(v) => value_to_result(v),
            Err(e) => flute_err_to_result(e),
        })
    }
```

- [ ] **Step 3: Append tests**

Append to `tests/tools_with_mock.rs`:

```rust
use flute_webhooks_mcp::server::{DeliveriesList, DeliveryId};

#[tokio::test]
async fn deliveries_list_no_filters() {
    let mock = MockRunner::new(vec![Ok(json!({"items": [], "total": 0}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server.deliveries_list(Parameters(DeliveriesList {
        endpoint_id: None, status: None, limit: None,
    })).await.unwrap();
    assert_eq!(mock.calls()[0], vec![
        "--profile", "sandbox", "--output", "json",
        "webhooks", "deliveries", "list",
    ]);
}

#[tokio::test]
async fn deliveries_list_all_filters() {
    let mock = MockRunner::new(vec![Ok(json!({"items": [], "total": 0}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server.deliveries_list(Parameters(DeliveriesList {
        endpoint_id: Some("e1".into()),
        status: Some("failed".into()),
        limit: Some(10),
    })).await.unwrap();
    assert_eq!(mock.calls()[0], vec![
        "--profile", "sandbox", "--output", "json",
        "webhooks", "deliveries", "list",
        "--endpoint-id", "e1",
        "--status", "failed",
        "--limit", "10",
    ]);
}

#[tokio::test]
async fn deliveries_get_argv() {
    let mock = MockRunner::new(vec![Ok(json!({"id":"d1"}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server.deliveries_get(Parameters(DeliveryId { id: "d1".into() })).await.unwrap();
    assert_eq!(mock.calls()[0], vec![
        "--profile", "sandbox", "--output", "json",
        "webhooks", "deliveries", "get", "d1",
    ]);
}

#[tokio::test]
async fn deliveries_retry_argv() {
    let mock = MockRunner::new(vec![Ok(json!({"id":"d1","status":"pending"}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server.deliveries_retry(Parameters(DeliveryId { id: "d1".into() })).await.unwrap();
    assert_eq!(mock.calls()[0], vec![
        "--profile", "sandbox", "--output", "json",
        "webhooks", "deliveries", "retry", "d1",
    ]);
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add src/server.rs tests/tools_with_mock.rs
git commit -m "feat: deliveries_{list,get,retry} tools"
```

---

## Task 12: `auth_status`

**Files:**
- Modify: `src/server.rs`
- Modify: `tests/tools_with_mock.rs`

`auth token` emits the bearer JWT as a single line of text (not JSON). We don't expose the JWT through MCP; we only translate "command succeeded" → `authenticated: true`.

- [ ] **Step 1: Add the tool method**

Inside the `#[tool_router]` block:

```rust
    #[tool(description = "Check whether credentials are present for the active profile. Returns `{authenticated, profile}`. Does NOT return the JWT.")]
    pub async fn auth_status(
        &self,
        _params: Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        // `auth token` is the cheapest way to test credentials. It emits plain text
        // on success, which the runner converts to a Decode error; that's fine,
        // we treat success-or-Decode-after-success as authenticated, Auth as not.
        let mut args = self.base_args();
        args.extend(["auth".into(), "token".into()]);
        let profile = self.config.profile.as_cli_str();
        let payload = match self.runner.run(&args).await {
            Ok(_) | Err(FluteError::Decode { .. }) => serde_json::json!({
                "authenticated": true, "profile": profile,
            }),
            Err(FluteError::Auth { .. }) => serde_json::json!({
                "authenticated": false, "profile": profile,
                "message": "Run `flute-webhook auth login` (optionally with --profile)",
            }),
            Err(e) => return Ok(flute_err_to_result(e)),
        };
        Ok(value_to_result(payload))
    }
```

- [ ] **Step 2: Append tests**

Append to `tests/tools_with_mock.rs`:

```rust
#[tokio::test]
async fn auth_status_reports_authenticated_when_decode_ok() {
    use flute_webhooks_mcp::error::FluteError;
    // ProcessRunner would return Decode for the plain-text JWT — treat as authenticated.
    let mock = MockRunner::new(vec![Err(FluteError::Decode {
        message: "expected JSON, got text".into(),
    })]);
    let server = FluteServer::new(cfg(), mock.clone());
    let result = server.auth_status(Parameters(Empty {})).await.unwrap();
    let content = result.content.expect("expected content");
    let first = content.first().expect("expected at least one content item");
    let json = first.as_text().expect("expected text content").text.as_str();
    assert!(json.contains("\"authenticated\":true"), "got {json}");
    assert!(json.contains("\"sandbox\""), "got {json}");
}

#[tokio::test]
async fn auth_status_reports_unauth_on_kind_auth() {
    use flute_webhooks_mcp::error::FluteError;
    let mock = MockRunner::new(vec![Err(FluteError::Auth {
        message: "no credentials for [sandbox]".into(),
    })]);
    let server = FluteServer::new(cfg(), mock.clone());
    let result = server.auth_status(Parameters(Empty {})).await.unwrap();
    let content = result.content.expect("expected content");
    let first = content.first().expect("expected at least one content item");
    let json = first.as_text().expect("expected text content").text.as_str();
    assert!(json.contains("\"authenticated\":false"), "got {json}");
    assert!(json.contains("auth login"), "got {json}");
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: green.

- [ ] **Step 4: Commit**

```bash
git add src/server.rs tests/tools_with_mock.rs
git commit -m "feat: auth_status tool"
```

---

## Task 13: `main.rs` wire-up + clippy/fmt

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Replace `src/main.rs`**

```rust
use std::sync::Arc;

use clap::Parser;
use flute_webhooks_mcp::{
    config::{Config, ConfigError},
    runner::ProcessRunner,
    server::FluteServer,
};
use rmcp::{transport::io::stdio, ServiceExt};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "flute-webhooks-mcp", about = "MCP server for the flute-webhook CLI")]
struct Args {
    /// Override `FLUTE_PROFILE` (sandbox | production).
    #[arg(long, env = "FLUTE_PROFILE")]
    profile: Option<String>,
    /// Override `FLUTE_WEBHOOK_BIN`.
    #[arg(long, env = "FLUTE_WEBHOOK_BIN")]
    binary: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // tracing → stderr ONLY (stdout is owned by the MCP transport).
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    // CLI args override env by re-injecting into the env-var lookup the Config sees.
    let cfg = Config::from_env(|name| match name {
        "FLUTE_PROFILE" => args.profile.clone().or_else(|| std::env::var(name).ok()),
        "FLUTE_WEBHOOK_BIN" => args.binary.clone().or_else(|| std::env::var(name).ok()),
        other => std::env::var(other).ok(),
    });

    let cfg = match cfg {
        Ok(c) => c,
        Err(ConfigError::BinaryNotFound) => {
            eprintln!("flute-webhooks-mcp: could not find `flute-webhook` on PATH. \
                       Install it from https://github.com/getflute/flute-webhooks or set FLUTE_WEBHOOK_BIN.");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("flute-webhooks-mcp: configuration error: {e}");
            std::process::exit(2);
        }
    };

    tracing::info!(
        profile = %cfg.profile.as_cli_str(),
        binary = %cfg.binary.display(),
        "starting flute-webhooks-mcp"
    );

    let runner = Arc::new(ProcessRunner {
        binary: cfg.binary.clone(),
        timeout: cfg.timeout,
        debug: cfg.debug,
    });
    let server = FluteServer::new(Arc::new(cfg), runner);

    // Drive stdio transport; .waiting().await blocks until the client disconnects.
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
```

> The line `let service = server.serve(stdio()).await?;` calls into `rmcp::ServiceExt`. If that exact method signature doesn't compile against the installed rmcp version, check `cargo doc --open -p rmcp` for the current `ServiceExt` API — the pattern (transport in, service handle out, await `.waiting()`) is stable.

- [ ] **Step 2: Add `anyhow` to `Cargo.toml`**

Append to `[dependencies]` in `Cargo.toml`:

```toml
anyhow = "1"
```

- [ ] **Step 3: Build the binary**

Run: `cargo build`
Expected: clean compile.

- [ ] **Step 4: Run clippy and fmt**

Run: `cargo clippy --all-targets --no-deps -- -D warnings`
Expected: clean.

Run: `cargo fmt --all -- --check`
Expected: clean (run `cargo fmt --all` if anything is reformatted).

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: green.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs
git commit -m "feat: wire main with stdio transport and clap overrides"
```

---

## Task 14: End-to-end stdio test

**Files:**
- Create: `tests/e2e_stdio.rs`

- [ ] **Step 1: Write the test**

```rust
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use tempfile::TempDir;

fn write_fake_flute(dir: &TempDir, stdout: &str, exit_code: i32) -> std::path::PathBuf {
    let path = dir.path().join("flute-webhook");
    let script = format!(
        "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{}'\nexit {}\n",
        stdout.replace('\'', "'\\''"),
        exit_code,
    );
    fs::write(&path, script).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

fn jsonrpc(id: u64, method: &str, params: Value) -> String {
    let frame = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    format!("{}\n", serde_json::to_string(&frame).unwrap())
}

fn read_one_frame(reader: &mut BufReader<impl std::io::Read>) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read frame");
    serde_json::from_str(line.trim()).expect("parse frame")
}

#[test]
fn lists_tools_and_calls_endpoints_list_through_stdio() {
    let dir = TempDir::new().unwrap();
    let fake = write_fake_flute(&dir, r#"{"data":[{"id":"e1"}]}"#, 0);

    let bin = assert_cmd::cargo::cargo_bin("flute-webhooks-mcp");
    let mut child = Command::new(bin)
        .env("FLUTE_WEBHOOK_BIN", &fake)
        .env("FLUTE_PROFILE", "sandbox")
        .env("RUST_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn server");

    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    // 1. initialize
    stdin.write_all(jsonrpc(1, "initialize", json!({
        "protocolVersion":"2025-03-26",
        "capabilities":{},
        "clientInfo":{"name":"e2e","version":"0"}
    })).as_bytes()).unwrap();
    let init = read_one_frame(&mut reader);
    assert_eq!(init["id"], 1);
    assert!(init["result"].is_object());

    // 2. notifications/initialized (one-way, no response)
    stdin.write_all(jsonrpc(0, "notifications/initialized", json!({})).as_bytes()).unwrap();

    // 3. tools/list — assert our tools are exposed
    stdin.write_all(jsonrpc(2, "tools/list", json!({})).as_bytes()).unwrap();
    let listed = read_one_frame(&mut reader);
    let names: Vec<String> = listed["result"]["tools"].as_array().unwrap()
        .iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for expected in &[
        "endpoints_list", "endpoints_get", "endpoints_create", "endpoints_update",
        "endpoints_delete", "endpoints_ping", "event_types_list",
        "deliveries_list", "deliveries_get", "deliveries_retry", "auth_status",
    ] {
        assert!(names.contains(&expected.to_string()), "missing tool {expected} in {names:?}");
    }

    // 4. tools/call endpoints_list — round-trip the payload
    stdin.write_all(jsonrpc(3, "tools/call", json!({
        "name":"endpoints_list","arguments":{}
    })).as_bytes()).unwrap();
    let called = read_one_frame(&mut reader);
    assert_eq!(called["id"], 3);
    let content = &called["result"]["content"][0];
    let text = content["text"].as_str().unwrap();
    assert!(text.contains("\"e1\""), "got {text}");

    // Cleanup
    let _ = child.kill();
    let _ = child.wait();
}
```

- [ ] **Step 2: Add the auth-error E2E case**

Append to `tests/e2e_stdio.rs`:

```rust
#[test]
fn auth_error_surfaces_as_is_error() {
    let dir = TempDir::new().unwrap();
    let fake = write_fake_flute(
        &dir,
        r#"{"kind":"auth","message":"no credentials for [sandbox]"}"#,
        1,
    );
    let bin = assert_cmd::cargo::cargo_bin("flute-webhooks-mcp");
    let mut child = Command::new(bin)
        .env("FLUTE_WEBHOOK_BIN", &fake)
        .env("FLUTE_PROFILE", "sandbox")
        .env("RUST_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    stdin.write_all(jsonrpc(1, "initialize", json!({
        "protocolVersion":"2025-03-26","capabilities":{},
        "clientInfo":{"name":"e2e","version":"0"}
    })).as_bytes()).unwrap();
    let _ = read_one_frame(&mut reader);
    stdin.write_all(jsonrpc(0, "notifications/initialized", json!({})).as_bytes()).unwrap();

    stdin.write_all(jsonrpc(2, "tools/call", json!({
        "name":"endpoints_list","arguments":{}
    })).as_bytes()).unwrap();
    let called = read_one_frame(&mut reader);
    assert_eq!(called["result"]["isError"], json!(true));
    let text = called["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"auth\""), "got {text}");
    assert!(text.contains("auth login"), "got {text}");

    let _ = child.kill();
    let _ = child.wait();
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test --test e2e_stdio -- --test-threads=1`
Expected: 2 tests pass.

> If the rmcp protocol version above is rejected by the installed rmcp, set it to whatever `rmcp::model::PROTOCOL_VERSION` reports (visible via a tiny print in the binary or in the crate docs). The test only needs *some* accepted handshake.

- [ ] **Step 4: Commit**

```bash
git add tests/e2e_stdio.rs
git commit -m "test: e2e stdio handshake + tools/list + tools/call + auth error"
```

---

## Task 15: `readme.md`

**Files:**
- Create: `readme.md`

- [ ] **Step 1: Write the readme**

```markdown
# flute-webhooks-mcp

An MCP (Model Context Protocol) server that lets an AI agent drive Aurora's `flute-webhook` CLI.

## How it works

The server spawns `flute-webhook --output json …` once per tool call, parses the structured stdout, and surfaces both successes and the upstream CLI's `{kind, message, status?, correlation_id?}` error envelope through MCP. Auth lives in the OS keychain on the operator's machine; this server never touches credentials directly.

## Install

```bash
cargo install --path .
```

Prereq: install `flute-webhook` first (see [getflute/flute-webhooks](https://github.com/getflute/flute-webhooks)) and run `flute-webhook auth login` once per profile you'll use.

## Run

```bash
flute-webhooks-mcp        # talks JSON-RPC over stdio
```

Start one server instance per environment (`sandbox` vs `production`) — the profile is **pinned at startup**.

## Environment variables

| Variable | Default | Purpose |
|---|---|---|
| `FLUTE_PROFILE` | `sandbox` | `sandbox` or `production` (alias `prod`). Pinned at startup. |
| `FLUTE_WEBHOOK_BIN` | resolved on `PATH` | Override the `flute-webhook` binary location. |
| `FLUTE_MCP_TIMEOUT_SECS` | `30` | Per-call timeout for the child process. |
| `FLUTE_MCP_DEBUG` | unset | When set to any non-empty value, route `flute-webhook` stderr to this server's tracing layer. |
| `RUST_LOG` | `info` | Standard `tracing` filter. Logs go to *stderr* only. |

## Claude Desktop config

```jsonc
{
  "mcpServers": {
    "flute-webhooks-sandbox": {
      "command": "flute-webhooks-mcp",
      "env": { "FLUTE_PROFILE": "sandbox" }
    },
    "flute-webhooks-prod": {
      "command": "flute-webhooks-mcp",
      "env": { "FLUTE_PROFILE": "production" }
    }
  }
}
```

## Tool inventory

| Tool | Idempotent? |
|---|---|
| `endpoints_list` | yes |
| `endpoints_get` | yes |
| `endpoints_create` | **no** — duplicates create a second endpoint |
| `endpoints_update` | yes (full-state PUT) |
| `endpoints_delete` | yes (second call returns 404) |
| `endpoints_ping` | yes |
| `event_types_list` | yes |
| `deliveries_list` | yes |
| `deliveries_get` | yes |
| `deliveries_retry` | **no** — each call schedules another retry |
| `auth_status` | yes |

Excluded by design: the upstream `tui`, `auth login` (interactive), `listen` (long-running, no JSON), and `update` (operator-only).

## Errors

Every tool returns either a success result or `isError: true` with a structured JSON content item containing at minimum a `kind` field — one of `api`, `transport`, `auth`, `decode`, `client`, `spawn`, `timeout`, `bad_output`. `api` errors also carry `status` and (where the server provided one) `correlation_id`.

For an agent: branch on `kind` first. `transport` and `api` with status ∈ {500,502,503,504} are safe to retry with backoff. `auth` means run `flute-webhook auth login` on the operator's machine.

## License

MIT.
```

- [ ] **Step 2: Commit**

```bash
git add readme.md
git commit -m "docs: operator README with env vars, Claude Desktop config, tool table"
```

---

## Task 16: Final verification

- [ ] **Step 1: Full test suite**

Run: `cargo test --all-targets`
Expected: green.

- [ ] **Step 2: Clippy across the workspace**

Run: `cargo clippy --all-targets --no-deps -- -D warnings`
Expected: clean.

- [ ] **Step 3: Formatting check**

Run: `cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 4: Release build**

Run: `cargo build --release`
Expected: produces `target/release/flute-webhooks-mcp`.

- [ ] **Step 5: Manual smoke test against the real CLI (if installed and credentialed)**

Run:
```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"endpoints_list","arguments":{}}}' | ./target/release/flute-webhooks-mcp
```
Expected: three JSON frames on stdout (initialize result, no-frame notification, tools/call result). If the operator hasn't run `flute-webhook auth login`, the third frame is `isError: true` with `kind: "auth"` — also a pass.

- [ ] **Step 6: Final commit if anything changed**

```bash
git status
# if anything to commit:
git add -A
git commit -m "chore: post-verification cleanup"
```

---

## Done criteria

All success criteria from the spec are satisfied:

1. `cargo build --release` produces the binary. ✅
2. `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check` are all clean. ✅
3. Eleven tools listed in `tools/list` over stdio. ✅
4. `kind:"auth"` errors surface as `isError: true` with remediation hint. ✅
5. `readme.md` documents env vars, the eleven tools, and a Claude Desktop snippet. ✅
