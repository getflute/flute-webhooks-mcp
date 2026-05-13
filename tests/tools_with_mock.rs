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

    use flute_webhooks_mcp::server::Empty;
    use rmcp::handler::server::wrapper::Parameters;
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
