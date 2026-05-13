use std::sync::Arc;

use rmcp::{
    ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, ErrorData as McpError, ServerInfo},
    schemars, tool, tool_handler, tool_router,
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
}

impl FluteServer {
    pub fn new(config: Arc<Config>, runner: Arc<dyn CliRunner>) -> Self {
        Self { config, runner }
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
        FluteError::Api {
            status,
            message,
            correlation_id,
        } => serde_json::json!({
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
        FluteError::BadOutput {
            exit_code,
            stdout,
            stderr,
        } => {
            let stderr_trunc = if stderr.len() > 4096 {
                &stderr[..4096]
            } else {
                stderr.as_str()
            };
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
#[serde(deny_unknown_fields)]
pub struct Empty {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EndpointId {
    /// The webhook endpoint id (from `endpoints_list`).
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct EndpointUpdate {
    pub id: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub events: Option<Vec<String>>,
    #[serde(default)]
    pub name: Option<String>,
    /// One of "active" | "inactive".
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliveriesList {
    #[serde(default)]
    pub endpoint_id: Option<String>,
    /// "success" or "failed".
    #[serde(default)]
    pub status: Option<String>,
    /// 1..=200 (server default 50).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliveryId {
    pub id: String,
}

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

    #[tool(description = "Get a single webhook endpoint by id. Safe to retry.")]
    pub async fn endpoints_get(
        &self,
        Parameters(p): Parameters<EndpointId>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend(["webhooks".into(), "endpoints".into(), "get".into(), p.id]);
        Ok(match self.run_cli(args).await {
            Ok(v) => value_to_result(v),
            Err(e) => flute_err_to_result(e),
        })
    }

    #[tool(
        description = "Create a new webhook endpoint. NOT idempotent — duplicates create a second endpoint. Response includes a one-shot `secret` you must store; the API never returns it again."
    )]
    pub async fn endpoints_create(
        &self,
        Parameters(p): Parameters<EndpointCreate>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend([
            "webhooks".into(),
            "endpoints".into(),
            "create".into(),
            "--url".into(),
            p.url,
            "--events".into(),
            p.events.join(","),
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
        args.extend(["webhooks".into(), "endpoints".into(), "update".into(), p.id]);
        if let Some(url) = p.url {
            args.extend(["--url".into(), url]);
        }
        if let Some(events) = p.events {
            args.extend(["--events".into(), events.join(",")]);
        }
        if let Some(name) = p.name {
            args.extend(["--name".into(), name]);
        }
        if let Some(status) = p.status {
            args.extend(["--status".into(), status]);
        }
        Ok(match self.run_cli(args).await {
            Ok(v) => value_to_result(v),
            Err(e) => flute_err_to_result(e),
        })
    }

    #[tool(
        description = "Delete a webhook endpoint by id. Second call returns 404; treat as idempotent."
    )]
    pub async fn endpoints_delete(
        &self,
        Parameters(p): Parameters<EndpointId>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend([
            "webhooks".into(),
            "endpoints".into(),
            "delete".into(),
            p.id.clone(),
            "--yes".into(),
        ]);
        Ok(match self.run_cli(args).await {
            Ok(_) => value_to_result(serde_json::json!({"deleted": true, "id": p.id})),
            Err(e) => flute_err_to_result(e),
        })
    }

    #[tool(
        description = "Send a test ping to a webhook endpoint. One-shot HTTP test, safe to retry."
    )]
    pub async fn endpoints_ping(
        &self,
        Parameters(p): Parameters<EndpointId>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend(["webhooks".into(), "endpoints".into(), "ping".into(), p.id]);
        Ok(match self.run_cli(args).await {
            Ok(v) => value_to_result(v),
            Err(e) => flute_err_to_result(e),
        })
    }

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

    #[tool(
        description = "List delivery log entries, optionally filtered by endpoint, status, and limit. Safe to retry."
    )]
    pub async fn deliveries_list(
        &self,
        Parameters(p): Parameters<DeliveriesList>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        args.extend(["webhooks".into(), "deliveries".into(), "list".into()]);
        if let Some(eid) = p.endpoint_id {
            args.extend(["--endpoint-id".into(), eid]);
        }
        if let Some(s) = p.status {
            args.extend(["--status".into(), s]);
        }
        if let Some(n) = p.limit {
            args.extend(["--limit".into(), n.to_string()]);
        }
        Ok(match self.run_cli(args).await {
            Ok(v) => value_to_result(v),
            Err(e) => flute_err_to_result(e),
        })
    }

    #[tool(
        description = "Get a single delivery log with full request and response bodies. Safe to retry."
    )]
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

    #[tool(
        description = "Re-schedule a failed delivery. NOT idempotent — each call schedules an additional retry. Check `deliveries_get` before retrying again."
    )]
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

    #[tool(
        description = "Check whether credentials are present for the active profile. Returns `{authenticated, profile}`. Does NOT return the JWT."
    )]
    pub async fn auth_status(
        &self,
        _params: Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
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
