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
