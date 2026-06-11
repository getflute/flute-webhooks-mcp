use std::sync::Arc;

use clap::Parser;
use flute_webhooks_mcp::{
    config::{Config, ConfigError},
    runner::ProcessRunner,
    server::FluteServer,
};
use rmcp::{ServiceExt, transport::io::stdio};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "flute-webhooks-mcp",
    about = "MCP server for the flute-webhooks CLI"
)]
struct Args {
    /// Override `FLUTE_PROFILE` (sandbox | production).
    #[arg(long, env = "FLUTE_PROFILE")]
    profile: Option<String>,
    /// Override `FLUTE_WEBHOOKS_BIN`.
    #[arg(long, env = "FLUTE_WEBHOOKS_BIN")]
    binary: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    let cfg = Config::from_env(|name| match name {
        "FLUTE_PROFILE" => args.profile.clone().or_else(|| std::env::var(name).ok()),
        "FLUTE_WEBHOOKS_BIN" => args.binary.clone().or_else(|| std::env::var(name).ok()),
        other => std::env::var(other).ok(),
    });

    let cfg = match cfg {
        Ok(c) => c,
        Err(ConfigError::BinaryNotFound) => {
            eprintln!(
                "flute-webhooks-mcp: could not find `flute-webhooks` on PATH. \
                       Install it from https://github.com/getflute/flute-webhooks-cli or set FLUTE_WEBHOOKS_BIN."
            );
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

    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
