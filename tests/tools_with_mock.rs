use std::sync::Arc;

use flute_webhooks_mcp::config::{Config, Profile};
use flute_webhooks_mcp::runner::MockRunner;
use flute_webhooks_mcp::server::{
    DeliveriesList, DeliveryId, Empty, EndpointCreate, EndpointId, EndpointUpdate, FluteServer,
};
use pretty_assertions::assert_eq;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

fn cfg() -> Arc<Config> {
    Arc::new(Config {
        profile: Profile::Sandbox,
        binary: PathBuf::from("/dev/null"),
        timeout: Duration::from_secs(5),
        debug: false,
    })
}

/// Pull the single JSON content item out of a CallToolResult as a string.
fn auth_payload(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .first()
        .expect("expected at least one content item")
        .as_text()
        .expect("expected text content")
        .text
        .clone()
}

#[tokio::test]
async fn endpoints_list_argv_matches_agents_md_spec() {
    let mock = MockRunner::new(vec![Ok(json!({"data": []}))]);
    let server = FluteServer::new(cfg(), mock.clone());

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
    // result.is_error is Option<bool>
    assert_eq!(result.is_error.unwrap_or(false), false);
}

#[tokio::test]
async fn endpoints_get_argv() {
    let mock = MockRunner::new(vec![Ok(json!({"id":"e1"}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server
        .endpoints_get(Parameters(EndpointId { id: "e1".into() }))
        .await
        .unwrap();
    assert_eq!(
        mock.calls()[0],
        vec![
            "--profile",
            "sandbox",
            "--output",
            "json",
            "webhooks",
            "endpoints",
            "get",
            "e1",
        ]
    );
}

#[tokio::test]
async fn endpoints_create_argv_with_name() {
    let mock = MockRunner::new(vec![Ok(json!({"id":"e1","secret":"shh"}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server
        .endpoints_create(Parameters(EndpointCreate {
            url: "https://x.example/hook".into(),
            events: vec![
                "transaction.card.captured".into(),
                "refund.completed".into(),
            ],
            name: Some("My Hook".into()),
        }))
        .await
        .unwrap();
    assert_eq!(
        mock.calls()[0],
        vec![
            "--profile",
            "sandbox",
            "--output",
            "json",
            "webhooks",
            "endpoints",
            "create",
            "--url",
            "https://x.example/hook",
            "--events",
            "transaction.card.captured,refund.completed",
            "--name",
            "My Hook",
        ]
    );
}

#[tokio::test]
async fn endpoints_update_only_includes_set_fields() {
    let mock = MockRunner::new(vec![Ok(json!({"id":"e1"}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server
        .endpoints_update(Parameters(EndpointUpdate {
            id: "e1".into(),
            url: None,
            events: None,
            name: None,
            status: Some("inactive".into()),
        }))
        .await
        .unwrap();
    assert_eq!(
        mock.calls()[0],
        vec![
            "--profile",
            "sandbox",
            "--output",
            "json",
            "webhooks",
            "endpoints",
            "update",
            "e1",
            "--status",
            "inactive",
        ]
    );
}

#[tokio::test]
async fn endpoints_delete_synthesizes_result() {
    let mock = MockRunner::new(vec![Ok(serde_json::Value::Null)]);
    let server = FluteServer::new(cfg(), mock.clone());
    let result = server
        .endpoints_delete(Parameters(EndpointId { id: "e1".into() }))
        .await
        .unwrap();
    assert_eq!(
        mock.calls()[0],
        vec![
            "--profile",
            "sandbox",
            "--output",
            "json",
            "webhooks",
            "endpoints",
            "delete",
            "e1",
            "--yes",
        ]
    );
    let content = &result.content;
    let first = content.first().expect("expected at least one content item");
    let json = first
        .as_text()
        .expect("expected text content")
        .text
        .as_str();
    assert!(json.contains("\"deleted\""), "missing deleted: {json}");
    assert!(json.contains("\"e1\""), "missing id: {json}");
}

#[tokio::test]
async fn auth_error_pass_through_is_error() {
    use flute_webhooks_mcp::error::FluteError;
    let mock = MockRunner::new(vec![Err(FluteError::Auth {
        message: "no credentials for [sandbox]".into(),
    })]);
    let server = FluteServer::new(cfg(), mock.clone());
    let result = server.endpoints_list(Parameters(Empty {})).await.unwrap();
    assert_eq!(result.is_error.unwrap_or(false), true);
    let content = &result.content;
    let first = content.first().expect("expected at least one content item");
    let json = first
        .as_text()
        .expect("expected text content")
        .text
        .as_str();
    assert!(json.contains("\"auth\""), "missing kind=auth: {json}");
    assert!(
        json.contains("auth login"),
        "missing remediation hint: {json}"
    );
}

#[tokio::test]
async fn event_types_list_argv() {
    let mock = MockRunner::new(vec![Ok(json!({"data": []}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server.event_types_list(Parameters(Empty {})).await.unwrap();
    assert_eq!(
        mock.calls()[0],
        vec![
            "--profile",
            "sandbox",
            "--output",
            "json",
            "webhooks",
            "event-types",
            "list",
        ]
    );
}

#[tokio::test]
async fn deliveries_list_no_filters() {
    let mock = MockRunner::new(vec![Ok(json!({
        "items": [],
        "pageInfo": {"pageIndex": 0, "pageSize": 50, "totalItems": 0, "totalPages": 0, "hasMore": false}
    }))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server
        .deliveries_list(Parameters(DeliveriesList {
            endpoint_id: None,
            status: None,
            limit: None,
        }))
        .await
        .unwrap();
    assert_eq!(
        mock.calls()[0],
        vec![
            "--profile",
            "sandbox",
            "--output",
            "json",
            "webhooks",
            "deliveries",
            "list",
        ]
    );
}

#[tokio::test]
async fn deliveries_list_all_filters() {
    let mock = MockRunner::new(vec![Ok(json!({
        "items": [],
        "pageInfo": {"pageIndex": 0, "pageSize": 50, "totalItems": 0, "totalPages": 0, "hasMore": false}
    }))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server
        .deliveries_list(Parameters(DeliveriesList {
            endpoint_id: Some("e1".into()),
            status: Some("failed".into()),
            limit: Some(10),
        }))
        .await
        .unwrap();
    assert_eq!(
        mock.calls()[0],
        vec![
            "--profile",
            "sandbox",
            "--output",
            "json",
            "webhooks",
            "deliveries",
            "list",
            "--endpoint-id",
            "e1",
            "--status",
            "failed",
            "--limit",
            "10",
        ]
    );
}

#[tokio::test]
async fn deliveries_get_argv() {
    let mock = MockRunner::new(vec![Ok(json!({"id":"d1"}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server
        .deliveries_get(Parameters(DeliveryId { id: "d1".into() }))
        .await
        .unwrap();
    assert_eq!(
        mock.calls()[0],
        vec![
            "--profile",
            "sandbox",
            "--output",
            "json",
            "webhooks",
            "deliveries",
            "get",
            "d1",
        ]
    );
}

#[tokio::test]
async fn deliveries_retry_argv() {
    let mock = MockRunner::new(vec![Ok(json!({
        "deliveryLogId": "d1",
        "endpointId": "e1",
        "eventId": "ev1",
        "eventType": "transaction.card.captured",
        "attemptNumber": 2,
        "deliveryLogStatus": "Failure",
        "endpointHTTPResponseCode": 500,
        "roundTripDurationMs": 12,
        "requestBody": "{}",
        "responseBody": "err"
    }))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server
        .deliveries_retry(Parameters(DeliveryId { id: "d1".into() }))
        .await
        .unwrap();
    assert_eq!(
        mock.calls()[0],
        vec![
            "--profile",
            "sandbox",
            "--output",
            "json",
            "webhooks",
            "deliveries",
            "retry",
            "d1",
        ]
    );
}

#[tokio::test]
async fn auth_status_reports_authenticated_when_decode_ok() {
    use flute_webhooks_mcp::error::FluteError;
    let mock = MockRunner::new(vec![Err(FluteError::Decode {
        message: "expected JSON, got text".into(),
    })]);
    let server = FluteServer::new(cfg(), mock.clone());
    let result = server.auth_status(Parameters(Empty {})).await.unwrap();
    let first = result
        .content
        .first()
        .expect("expected at least one content item");
    let json = first
        .as_text()
        .expect("expected text content")
        .text
        .as_str();
    assert!(json.contains("\"authenticated\":true"), "got {json}");
    assert!(json.contains("\"sandbox\""), "got {json}");
}

/// The real missing-credentials path. Upstream `auth_print_keys` builds this with a
/// bare `anyhow!`, and the CLI's classifier only lifts `kind` out of a downcast to
/// its `ApiError` — so it reaches us as `kind:"client"`, not `kind:"auth"`.
#[tokio::test]
async fn auth_status_reports_unauth_on_kind_client() {
    use flute_webhooks_mcp::error::FluteError;
    let mock = MockRunner::new(vec![Err(FluteError::Client {
        message: "no credentials for [sandbox]; run `flute-webhooks auth login`".into(),
    })]);
    let server = FluteServer::new(cfg(), mock.clone());
    let result = server.auth_status(Parameters(Empty {})).await.unwrap();
    assert!(
        !result.is_error.unwrap_or(false),
        "must not be a tool error"
    );
    let json = auth_payload(&result);
    assert!(json.contains("\"authenticated\":false"), "got {json}");
    assert!(json.contains("\"reason\":\"client\""), "got {json}");
    assert!(json.contains("no credentials for [sandbox]"), "got {json}");
    assert!(json.contains("auth login"), "got {json}");
}

/// An OAuth rejection is also `kind:"client"` upstream, and is equally
/// "not authenticated" — but the operator hint differs, so the CLI's own message
/// must survive into the payload rather than being replaced by a canned string.
#[tokio::test]
async fn auth_status_passes_through_oauth_failure_message() {
    use flute_webhooks_mcp::error::FluteError;
    let mock = MockRunner::new(vec![Err(FluteError::Client {
        message: "OAuth token request to https://oauth.example failed: 401 invalid_client".into(),
    })]);
    let server = FluteServer::new(cfg(), mock.clone());
    let result = server.auth_status(Parameters(Empty {})).await.unwrap();
    let json = auth_payload(&result);
    assert!(json.contains("\"authenticated\":false"), "got {json}");
    assert!(json.contains("invalid_client"), "got {json}");
}

/// Unreachable from `auth keys` today (`ApiError::Auth` is only built by the API
/// client), but wired up so an upstream reclassification keeps working.
#[tokio::test]
async fn auth_status_reports_unauth_on_kind_auth() {
    use flute_webhooks_mcp::error::FluteError;
    let mock = MockRunner::new(vec![Err(FluteError::Auth {
        message: "no token".into(),
    })]);
    let server = FluteServer::new(cfg(), mock.clone());
    let result = server.auth_status(Parameters(Empty {})).await.unwrap();
    let json = auth_payload(&result);
    assert!(json.contains("\"authenticated\":false"), "got {json}");
    assert!(json.contains("\"reason\":\"auth\""), "got {json}");
    assert!(json.contains("auth login"), "got {json}");
}

/// An infrastructure fault is not an authentication verdict — it must stay a tool
/// error instead of being reported as `authenticated:false`.
#[tokio::test]
async fn auth_status_surfaces_spawn_failure_as_tool_error() {
    use flute_webhooks_mcp::error::FluteError;
    let mock = MockRunner::new(vec![Err(FluteError::Spawn("no such file".into()))]);
    let server = FluteServer::new(cfg(), mock.clone());
    let result = server.auth_status(Parameters(Empty {})).await.unwrap();
    assert!(
        result.is_error.unwrap_or(false),
        "spawn must be a tool error"
    );
    let json = auth_payload(&result);
    assert!(json.contains("\"kind\":\"spawn\""), "got {json}");
    assert!(!json.contains("authenticated"), "got {json}");
}

#[tokio::test]
async fn endpoints_ping_argv() {
    let mock = MockRunner::new(vec![Ok(json!({
        "isDelivered": true,
        "endpointHTTPResponseCode": 200,
        "roundTripDurationMs": 34
    }))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server
        .endpoints_ping(Parameters(EndpointId { id: "e1".into() }))
        .await
        .unwrap();
    assert_eq!(
        mock.calls()[0],
        vec![
            "--profile",
            "sandbox",
            "--output",
            "json",
            "webhooks",
            "endpoints",
            "ping",
            "e1",
        ]
    );
}

#[tokio::test]
async fn auth_status_argv() {
    let mock = MockRunner::new(vec![Err(flute_webhooks_mcp::error::FluteError::Decode {
        message: "plain-text JWT".into(),
    })]);
    let server = FluteServer::new(cfg(), mock.clone());
    server.auth_status(Parameters(Empty {})).await.unwrap();
    assert_eq!(
        mock.calls()[0],
        vec!["--profile", "sandbox", "--output", "json", "auth", "keys",]
    );
}
