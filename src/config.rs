use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Uat,
    Production,
}

impl Profile {
    pub fn as_cli_str(self) -> &'static str {
        match self {
            Profile::Uat => "uat",
            Profile::Production => "production",
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid FLUTE_PROFILE value `{0}` (expected `uat` or `production`)")]
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
            None | Some("") | Some("uat") => Profile::Uat,
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

        Ok(Self {
            profile,
            binary,
            timeout,
            debug,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn make_env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
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
    fn defaults_to_uat_and_30s() {
        let dir = TempDir::new().unwrap();
        let bin = fake_binary(&dir, "flute-webhook");
        let pairs = [("FLUTE_WEBHOOK_BIN", bin.to_str().unwrap())];
        let env = make_env(&pairs);
        let cfg = Config::from_env(env).unwrap();
        assert_eq!(cfg.profile, Profile::Uat);
        assert_eq!(cfg.timeout, Duration::from_secs(30));
        assert!(!cfg.debug);
    }

    #[test]
    fn accepts_production_and_prod_alias() {
        let dir = TempDir::new().unwrap();
        let bin = fake_binary(&dir, "flute-webhook");
        for value in ["production", "prod"] {
            let pairs = [
                ("FLUTE_WEBHOOK_BIN", bin.to_str().unwrap()),
                ("FLUTE_PROFILE", value),
            ];
            let env = make_env(&pairs);
            let cfg = Config::from_env(env).unwrap();
            assert_eq!(cfg.profile, Profile::Production);
        }
    }

    #[test]
    fn rejects_unknown_profile() {
        let dir = TempDir::new().unwrap();
        let bin = fake_binary(&dir, "flute-webhook");
        let pairs = [
            ("FLUTE_WEBHOOK_BIN", bin.to_str().unwrap()),
            ("FLUTE_PROFILE", "staging"),
        ];
        let env = make_env(&pairs);
        assert!(matches!(
            Config::from_env(env),
            Err(ConfigError::InvalidProfile(s)) if s == "staging"
        ));
    }

    #[test]
    fn missing_binary_errors() {
        let pairs = [("FLUTE_WEBHOOK_BIN", "/nope/does/not/exist")];
        let env = make_env(&pairs);
        assert!(matches!(
            Config::from_env(env),
            Err(ConfigError::BinaryUnusable(_))
        ));
    }
}
