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
