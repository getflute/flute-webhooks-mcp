use std::sync::Arc;

use flute_webhooks_mcp::config::{Config, Profile};
use flute_webhooks_mcp::runner::MockRunner;
use flute_webhooks_mcp::server::{EndpointCreate, EndpointId, EndpointUpdate, Empty, FluteServer};
use pretty_assertions::assert_eq;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

fn cfg() -> Arc<Config> {
    Arc::new(Config {
        profile: Profile::Uat,
        binary: PathBuf::from("/dev/null"),
        timeout: Duration::from_secs(5),
        debug: false,
    })
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
            "uat".into(),
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
    server.endpoints_get(Parameters(EndpointId { id: "e1".into() })).await.unwrap();
    assert_eq!(mock.calls()[0], vec![
        "--profile", "uat", "--output", "json",
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
        "--profile", "uat", "--output", "json",
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
        "--profile", "uat", "--output", "json",
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
        "--profile", "uat", "--output", "json",
        "webhooks", "endpoints", "delete", "e1", "--yes",
    ]);
    let content = &result.content;
    let first = content.first().expect("expected at least one content item");
    let json = first.as_text().expect("expected text content").text.as_str();
    assert!(json.contains("\"deleted\""), "missing deleted: {json}");
    assert!(json.contains("\"e1\""), "missing id: {json}");
}

#[tokio::test]
async fn auth_error_pass_through_is_isError() {
    use flute_webhooks_mcp::error::FluteError;
    let mock = MockRunner::new(vec![Err(FluteError::Auth {
        message: "no credentials for [uat]".into(),
    })]);
    let server = FluteServer::new(cfg(), mock.clone());
    let result = server.endpoints_list(Parameters(Empty {})).await.unwrap();
    assert_eq!(result.is_error.unwrap_or(false), true);
    let content = &result.content;
    let first = content.first().expect("expected at least one content item");
    let json = first.as_text().expect("expected text content").text.as_str();
    assert!(json.contains("\"auth\""), "missing kind=auth: {json}");
    assert!(json.contains("auth login"), "missing remediation hint: {json}");
}

#[tokio::test]
async fn event_types_list_argv() {
    let mock = MockRunner::new(vec![Ok(json!({"data": []}))]);
    let server = FluteServer::new(cfg(), mock.clone());
    server.event_types_list(Parameters(Empty {})).await.unwrap();
    assert_eq!(mock.calls()[0], vec![
        "--profile", "uat", "--output", "json",
        "webhooks", "event-types", "list",
    ]);
}
