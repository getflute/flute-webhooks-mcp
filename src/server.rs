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
            "message": format!("{message} — run `flute-webhooks auth login`"),
        }),
        FluteError::Decode { message } => serde_json::json!({
            "kind": "decode", "message": message,
        }),
        FluteError::Client { message } => serde_json::json!({
            "kind": "client", "message": message,
        }),
        FluteError::Spawn(msg) => serde_json::json!({
            "kind": "spawn",
            "message": format!("could not spawn flute-webhooks — set FLUTE_WEBHOOKS_BIN or install the CLI ({msg})"),
        }),
        FluteError::Timeout { secs } => serde_json::json!({
            "kind": "timeout", "message": format!("flute-webhooks timed out after {secs}s"),
        }),
        FluteError::BadOutput {
            exit_code,
            stdout,
            stderr,
        } => {
            let stderr_trunc = if stderr.len() > 4096 {
                &stderr[..stderr.floor_char_boundary(4096)]
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
    CallToolResult::error(vec![
        Content::json(payload).expect("serde_json::Value is always JSON-serializable"),
    ])
}

fn value_to_result(value: Value) -> CallToolResult {
    CallToolResult::success(vec![
        Content::json(value).expect("serde_json::Value is always JSON-serializable"),
    ])
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
    /// Event-type wire strings to subscribe to — the `eventType` field from
    /// `event_types_list` (the catalog dropped its `name` field in CLI v0.7.0).
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
    /// 1..=100; the CLI sends 50 when this is omitted. Maps to the server's
    /// `pageSize`, which is capped at 100 — a higher value fails with `kind:"api"`
    /// `status:400`. There is no page-index control, so this bounds the *only*
    /// reachable page: when `pageInfo.hasMore` is true, raise this toward 100
    /// before concluding any rows are unreachable.
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
        description = "List webhook endpoints for the active profile. Returns a paginated envelope `{items, pageInfo}` — iterate `items`. The CLI requests pageSize=100 and cannot request a later page, so `pageInfo.hasMore == true` means endpoints exist that this call cannot reach. Pure read; safe to retry."
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
        description = "Create a new webhook endpoint. NOT idempotent — duplicates create a second endpoint; check `endpoints_list` first when recovering from an ambiguous timeout. Response includes a one-shot `hmacSecret` you must store; the API never returns it again."
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

    #[tool(
        description = "Update an existing webhook endpoint. Sends a PATCH with a JSON Merge Patch body carrying ONLY the fields you pass — every field you omit is left unchanged server-side, so there is no need to re-send current values to preserve them. Idempotent: repeated calls converge on the same state, safe to retry after an ambiguous timeout."
    )]
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
        description = "List delivery log entries, optionally filtered by endpoint, status, and limit. Returns a paginated envelope `{items, pageInfo}` — iterate `items`; the total across all matching rows is `pageInfo.totalItems`. `limit` maps to `pageSize` only (CLI default 50, server max 100) and there is no `pageIndex`, so only the first page is ever returned. On `pageInfo.hasMore == true`, raise `limit` up to 100 first; if it is still true at `limit: 100`, the remaining matches are genuinely unreachable — narrow the filters (`endpoint_id`, `status`) to bring the set under 100. Safe to retry."
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
        description = "Re-schedule a failed delivery. Returns the new attempt's log record as a `DeliveryLogDetailDto` — the same shape `deliveries_get` returns. NOT idempotent: each call schedules an additional retry, so check `deliveries_get` before retrying again. The server rejects retries with `kind:\"api\"` `status:400` for ping deliveries (synthetic) and for deliveries already in `Success`; pre-filter to `deliveryLogStatus == \"Failure\"` and `eventType != \"ping\"`."
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
        description = "Check whether the CLI can mint a bearer token for the active profile. Returns `{authenticated, profile}`, plus `reason` and `message` when false. Does NOT return the JWT itself. A false result means the CLI could not produce a token — it does NOT prove credentials are absent: the upstream `auth keys` path reports missing credentials, a rejected client_id/secret, and a network failure during the OAuth exchange all as the same `kind:\"client\"`, so read `message` to tell them apart before concluding the operator needs to run `auth login`."
    )]
    pub async fn auth_status(
        &self,
        _params: Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let mut args = self.base_args();
        // `auth keys` since CLI v0.7.1; `auth token` survives only as a hidden
        // deprecated alias. Emits a bare JWT, not JSON — hence the Decode arm below.
        args.extend(["auth".into(), "keys".into()]);
        let profile = self.config.profile.as_cli_str();
        // `auth keys` prints a bare JWT, so a *successful* call fails JSON parsing
        // and lands in Decode — that is the authenticated signal, not an error.
        //
        // Every failure mode of that path (unknown profile, keychain read error, no
        // stored credentials, OAuth non-2xx, OAuth network error) is built with a
        // bare `anyhow!` upstream, and the CLI's classifier only lifts `kind` out of
        // a downcast to its `ApiError`. So they all arrive as `kind:"client"`.
        // `kind:"auth"` is reachable only from the API client, which `auth keys`
        // never touches — it is matched here purely so a future upstream
        // reclassification keeps working.
        let payload = match self.runner.run(&args).await {
            Ok(_) | Err(FluteError::Decode { .. }) => serde_json::json!({
                "authenticated": true, "profile": profile,
            }),
            Err(FluteError::Client { message }) => serde_json::json!({
                "authenticated": false, "profile": profile,
                "reason": "client",
                "message": message,
                "hint": "If this says no credentials, run `flute-webhooks auth login` (optionally with --profile). Otherwise the credentials exist but the OAuth exchange did not succeed.",
            }),
            Err(FluteError::Auth { message }) => serde_json::json!({
                "authenticated": false, "profile": profile,
                "reason": "auth",
                "message": message,
                "hint": "Run `flute-webhooks auth login` (optionally with --profile)",
            }),
            // Spawn / Timeout / BadOutput / Transport / Api are infrastructure
            // faults, not an authentication verdict — surface them as tool errors
            // rather than claiming the profile is unauthenticated.
            Err(e) => return Ok(flute_err_to_result(e)),
        };
        Ok(value_to_result(payload))
    }
}

#[tool_handler]
impl ServerHandler for FluteServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::default().with_instructions(
            "Drives the `flute-webhooks` CLI. The active profile is pinned at server start; \
             launch one instance per environment (sandbox vs production). Credentials are read \
             from the OS keychain via `flute-webhooks auth login` — run that first.",
        )
    }
}
