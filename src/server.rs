use std::sync::Arc;

use rmcp::{
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, ErrorData as McpError, ServerInfo},
    schemars,
    tool, tool_handler, tool_router,
    ServerHandler,
};
use serde::Deserialize;
use serde_json::Value;

use crate::config::Config;
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
        ServerInfo::default().with_instructions(
            "Drives the `flute-webhook` CLI. The active profile is pinned at server start; \
             launch one instance per environment (uat vs production). Credentials are read \
             from the OS keychain via `flute-webhook auth login` — run that first.",
        )
    }
}
