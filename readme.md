# flute-webhooks-mcp

An MCP (Model Context Protocol) server that lets an AI agent drive Aurora's `flute-webhooks` CLI.

## How it works

The server spawns `flute-webhooks --output json …` once per tool call, parses the structured stdout, and surfaces both successes and the upstream CLI's `{kind, message, status?, correlation_id?}` error envelope through MCP. Auth lives in the OS keychain on the operator's machine; this server never touches credentials directly.

## Install

Pick whichever installer matches your platform — each one drops a `flute-webhooks-mcp` binary on your `PATH`.

```bash
# macOS / Linux (curl + sh)
curl -LsSf https://github.com/getflute/flute-webhooks-mcp/releases/latest/download/flute-webhooks-mcp-installer.sh | sh

# macOS / Linux (Homebrew — this repo serves as its own tap)
brew install getflute/flute-webhooks-mcp/flute-webhooks-mcp

# Windows (PowerShell)
irm https://github.com/getflute/flute-webhooks-mcp/releases/latest/download/flute-webhooks-mcp-installer.ps1 | iex
```

Or, to build from source:

```bash
cargo install --path .
```

Prereq: install `flute-webhooks` first (see [getflute/flute-webhooks-cli](https://github.com/getflute/flute-webhooks-cli)) and run `flute-webhooks auth login` once per profile you'll use.

## Run

```bash
flute-webhooks-mcp        # talks JSON-RPC over stdio
```

Start one server instance per environment (`sandbox` vs `production`) — the profile is **pinned at startup**.

## Environment variables

| Variable | Default | Purpose |
|---|---|---|
| `FLUTE_PROFILE` | `sandbox` | `sandbox` or `production` (alias `prod`). Pinned at startup. |
| `FLUTE_WEBHOOKS_BIN` | resolved on `PATH` | Override the `flute-webhooks` binary location. |
| `FLUTE_MCP_TIMEOUT_SECS` | `30` | Per-call timeout for the child process. |
| `FLUTE_MCP_DEBUG` | unset | When set to any non-empty value, route `flute-webhooks` stderr to this server's tracing layer. |
| `RUST_LOG` | `info` | Standard `tracing` filter. Logs go to *stderr* only. |

## Claude Desktop config

```jsonc
{
  "mcpServers": {
    "flute-webhooks-sandbox": {
      "command": "flute-webhooks-mcp",
      "env": { "FLUTE_PROFILE": "sandbox" }
    },
    "flute-webhooks-prod": {
      "command": "flute-webhooks-mcp",
      "env": { "FLUTE_PROFILE": "production" }
    }
  }
}
```

## Tool inventory

| Tool | Idempotent? |
|---|---|
| `endpoints_list` | yes |
| `endpoints_get` | yes |
| `endpoints_create` | **no** — duplicates create a second endpoint |
| `endpoints_update` | yes (full-state PUT) |
| `endpoints_delete` | yes (second call returns 404) |
| `endpoints_ping` | yes |
| `event_types_list` | yes |
| `deliveries_list` | yes |
| `deliveries_get` | yes |
| `deliveries_retry` | **no** — each call schedules another retry |
| `auth_status` | yes |

Excluded by design: the upstream `tui`, `auth login` (interactive), `listen` (long-running, no JSON), and `update` (operator-only).

## Errors

Every tool returns either a success result or `isError: true` with a structured JSON content item containing at minimum a `kind` field — one of `api`, `transport`, `auth`, `decode`, `client`, `spawn`, `timeout`, `bad_output`. `api` errors also carry `status` and (where the server provided one) `correlation_id`.

For an agent: branch on `kind` first. `transport` and `api` with status ∈ {500,502,503,504} are safe to retry with backoff. `auth` means run `flute-webhooks auth login` on the operator's machine.

## License

MIT.
