# flute-webhooks-mcp — design

**Date:** 2026-05-13
**Status:** approved (brainstorm)
**Author:** chad.lung@risewithaurora.com (with Claude)

## Goal

Build a Model Context Protocol (MCP) server in Rust that lets an AI agent
operate Aurora's Flute webhook control plane by driving the existing
`flute-webhook` CLI ([getflute/flute-webhooks], private). The CLI already
exposes a machine-readable contract (`--output json`, structured error
envelope) documented in its `AGENTS.md`; this project is the JSON-RPC
façade in front of it.

[getflute/flute-webhooks]: https://github.com/getflute/flute-webhooks

## Non-goals

- Re-implementing Flute's REST API in Rust. The upstream CLI owns HTTP,
  auth, retries, and DTOs; we shell out to it.
- Managing credentials. The operator runs `flute-webhook auth login`
  beforehand; the MCP server never touches the keychain.
- Exposing the TUI, the interactive `auth login`, the long-running
  `listen` forwarder, or the `update` subcommand. Those have no JSON
  surface or can't be driven non-interactively.
- Supporting transports other than stdio.
- Mixing UAT and production in one process.

## Key decisions (from brainstorm)

| Decision | Choice |
|---|---|
| Tool scope | Agent-safe subset only (no `tui`, `auth login`, `listen`, `update`). |
| Transport | stdio only. |
| Credentials | Assume operator has run `flute-webhook auth login`; surface `kind:"auth"` errors as structured tool errors. |
| Profile | Pinned at server start via `FLUTE_PROFILE` (default `uat`). One server instance per environment. |
| CLI invocation | One `flute-webhook` child process per tool call via `tokio::process::Command`. |

## Architecture

```
flute-webhooks-mcp  (single Rust binary, stdio MCP)

  ┌──────────┐    ┌──────────┐    ┌─────────────────────┐
  │  rmcp    │───►│  Tools   │───►│  CliRunner          │
  │  stdio   │    │  layer   │    │  (tokio::process)   │
  │  server  │◄───│          │◄───│                     │
  └──────────┘    └──────────┘    └─────────┬───────────┘
                       │                    │
                       ▼                    ▼
                 ┌──────────┐         spawns
                 │  Config  │         `flute-webhook
                 │  (start- │          --profile X
                 │   time)  │          --output json …`
                 └──────────┘
```

- **`main.rs`** wires tracing → stderr, loads `Config` from env, builds
  `CliRunner`, hands to rmcp stdio server, runs until stdin closes.
- **`config.rs`** holds `Config { profile, binary, timeout }`.
  - `FLUTE_PROFILE` (default `uat`)
  - `FLUTE_WEBHOOK_BIN` (default `which("flute-webhook")`; hard error at
    startup if neither resolves)
  - `FLUTE_MCP_TIMEOUT_SECS` (default `30`)
  - `FLUTE_MCP_DEBUG=1` routes child stderr to our tracing layer
- **`runner.rs`** defines `trait CliRunner { async fn run(&self, args: &[&str]) -> Result<Value, FluteError>; }`.
  - `ProcessRunner` spawns the child, applies the timeout, collects
    stdout, parses JSON, maps non-zero exits via the AGENTS.md envelope.
  - `MockRunner` records calls and returns canned values for tests.
- **`tools/`** — one module per subcommand family (`endpoints`,
  `event_types`, `deliveries`, `auth`). Each tool builds an argv,
  calls the runner, returns either a `Value` or a `FluteError`.
- **`error.rs`** — `FluteError` mirroring the upstream envelope plus
  our own `Spawn`, `Timeout`, `BadOutput`. `Into<rmcp::ErrorData>`
  preserves the structured fields.

The whole binary is ~1,000 LoC plus tests; every source file stays under
~400 LoC.

## Tool inventory

All tools are exposed under JSON-RPC `tools/call`. Output on success is
the upstream CLI's stdout JSON, unchanged. Output on failure is a
structured error (see Error handling).

| Tool | Wraps | Input | Output |
|---|---|---|---|
| `endpoints_list` | `webhooks endpoints list` | `{}` | `{ "data": [GetWebhookEndpointDto, …] }` |
| `endpoints_get` | `webhooks endpoints get <id>` | `{ id: string }` | `GetWebhookEndpointDto` |
| `endpoints_create` | `webhooks endpoints create …` | `{ url: string, events: string[], name?: string }` | `CreateWebhookEndpointResponse` (one-shot `secret`) |
| `endpoints_update` | `webhooks endpoints update <id> …` | `{ id: string, url?: string, events?: string[], name?: string, status?: "active"\|"inactive" }` | `GetWebhookEndpointDto` |
| `endpoints_delete` | `webhooks endpoints delete <id> --yes` | `{ id: string }` | `{ "deleted": true, "id": "<id>" }` (synthesized — CLI emits empty stdout) |
| `endpoints_ping` | `webhooks endpoints ping <id>` | `{ id: string }` | `PingResponseDto` |
| `event_types_list` | `webhooks event-types list` | `{}` | `{ "data": [EventTypeDto, …] }` |
| `deliveries_list` | `webhooks deliveries list …` | `{ endpoint_id?: string, status?: "success"\|"failed", limit?: integer 1–200 (default 50) }` | `ListDeliveryLogsDto` |
| `deliveries_get` | `webhooks deliveries get <id>` | `{ id: string }` | `DeliveryLogDetailDto` |
| `deliveries_retry` | `webhooks deliveries retry <id>` | `{ id: string }` | `DeliveryLogSummaryDto` |
| `auth_status` | `auth token` | `{}` | `{ authenticated: bool, profile: "uat"\|"production", expires_at?: string }` |

Idempotency hints (from AGENTS.md) live in each tool's `description`
field. `endpoints_create` and `deliveries_retry` get prominent
"NOT idempotent" warnings.

### Excluded by design

- `tui` — interactive, no JSON
- `auth login` — interactive prompt
- `listen` — long-running, no JSON
- `update` — operator-only

## Data flow (single tool call)

```
agent → MCP rmcp server
  receives `tools/call` { name: "endpoints_list", arguments: {} }
    │
    ▼
tool handler builds argv:
  ["--profile", "uat", "--output", "json", "webhooks", "endpoints", "list"]
    │
    ▼
CliRunner::run(argv)
  - tokio::process::Command::new(config.binary)
      .args(argv)
      .env_remove("RUST_LOG")
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .kill_on_drop(true)
  - tokio::time::timeout(config.timeout, child.wait_with_output())
  - exit success → parse stdout as Value → Ok(value)
    exit nonzero → parse envelope → Err(FluteError::from_envelope(…))
    parse failure → Err(FluteError::BadOutput { exit_code, stdout, stderr })
    │
    ▼
tool handler → CallToolResult { content: [Json(value)] }
```

## Error handling

`FluteError` → MCP mapping:

| Variant | Source | MCP surfacing |
|---|---|---|
| `Api { status, message, correlation_id }` | `kind: "api"` envelope | `isError: true`, content includes `{kind, status, correlation_id, message}` |
| `Transport { message }` | `kind: "transport"` | `isError: true`, retry-safe hint in description |
| `Auth { message }` | `kind: "auth"` | `isError: true`, message: "Run `flute-webhook auth login`" |
| `Decode { message }` | `kind: "decode"` | `isError: true` |
| `Client { message }` | `kind: "client"` | `isError: true` (our own bug — bad argv) |
| `Spawn { source }` | OS-level | `isError: true`, message: "could not spawn `flute-webhook` — set FLUTE_WEBHOOK_BIN or install the CLI" |
| `Timeout { secs }` | tokio timeout fires | `isError: true`; child killed via `kill_on_drop` |
| `BadOutput { exit_code, stdout, stderr }` | nonzero exit, unparseable stdout | `isError: true`, includes truncated stderr (max 4 KB) |

All variants pass through structured fields so an agent can branch on
`kind`/`status` as AGENTS.md prescribes — we don't flatten to a string.

stderr is captured but never returned on success. We don't pass
`--debug` to the child by default (would pollute stdout for non-TUI
commands per upstream docs). Setting `FLUTE_MCP_DEBUG=1` on our server
routes the child's stderr to our tracing layer.

## Concurrency

Each MCP tool call is a separate `tokio::spawn`. No shared mutable state
besides `Arc<Config>` (`Send + Sync`, frozen at startup). Multiple calls
in flight spawn multiple `flute-webhook` processes; each does its own
OAuth handshake internally.

## Crate choices

| Crate | Purpose |
|---|---|
| `rmcp` | Official Rust MCP SDK — `ServerHandler`, `#[tool]` derive, stdio transport. |
| `tokio` | Async runtime, `process`, `time::timeout`, `signal`. Features: `macros, rt-multi-thread, process, time, signal, io-util`. |
| `serde` / `serde_json` | Input structs and stdout parsing. `serde(deny_unknown_fields)` on inputs. |
| `schemars` | JSON Schema for tool inputs (rmcp consumes this). |
| `thiserror` | `FluteError`. |
| `tracing` + `tracing-subscriber` | Server logs to stderr (stdout is owned by MCP). |
| `which` | Locate `flute-webhook` on PATH with a clear startup error. |
| `clap` | Server's own startup flags. |
| **dev** | |
| `tokio` test feature | Async test harness. |
| `pretty_assertions` | Matches upstream style. |
| `tempfile` | Fake-binary fixtures. |
| `assert_cmd` | End-to-end stdio test. |

Deliberately **not** pulled in: `anyhow` (typed errors all the way),
`reqwest` and `keyring` (the CLI owns HTTP and credentials).

## Project layout

```
flute-webhooks-mcp/
├── Cargo.toml
├── readme.md                   env vars, tool list, Claude Desktop snippet
├── src/
│   ├── main.rs                 clap, tracing, build server, run
│   ├── lib.rs                  re-exports for tests
│   ├── config.rs               Config + Profile + env loading
│   ├── error.rs                FluteError + envelope deserializer
│   ├── runner.rs               CliRunner trait + ProcessRunner + MockRunner
│   ├── server.rs               rmcp ServerHandler glue, holds Arc<Config> + Arc<dyn CliRunner>
│   └── tools/
│       ├── mod.rs              module index + argv helpers
│       ├── endpoints.rs        endpoints_{list,get,create,update,delete,ping}
│       ├── event_types.rs      event_types_list
│       ├── deliveries.rs       deliveries_{list,get,retry}
│       └── auth.rs             auth_status
└── tests/
    ├── runner_unit.rs          ProcessRunner against a fake binary
    ├── tools_with_mock.rs      every tool against MockRunner — argv shape & error mapping
    └── e2e_stdio.rs            spawn our binary + fake flute-webhook, real MCP frames
```

## Testing strategy

Three layers:

1. **Unit — `ProcessRunner` against a fake binary** (`runner_unit.rs`).
   A small POSIX shell script (`.cmd` on Windows) in a `TempDir` echoes a
   canned JSON payload and exits 0/1. Cases: success body,
   `kind:"api"` envelope with `status` + `correlation_id`, malformed
   stdout (→ `BadOutput`), hung child that never writes (→ `Timeout`
   with a 250 ms test config), missing binary (→ `Spawn`).

2. **Tool — every tool against `MockRunner`** (`tools_with_mock.rs`).
   `MockRunner` records `argv` and returns a configurable
   `Result<Value, FluteError>`. Per tool: (a) happy path — assert argv
   matches the AGENTS.md spec verbatim, (b) error pass-through — `Api`
   envelope surfaces intact with `isError: true`, (c) input validation
   — e.g. `endpoints_create` missing `events` is rejected by schema
   before we spawn.

3. **End-to-end — real stdio MCP frames** (`e2e_stdio.rs`).
   `assert_cmd` launches our compiled binary with `FLUTE_WEBHOOK_BIN`
   pointing at the fake script. We hand-write the JSON-RPC handshake
   (`initialize`, `tools/list`, `tools/call`) on stdin and parse
   responses on stdout. One golden test plus one auth-error case.

**Out of scope for tests:** network mocks (`wiremock` — the upstream
CLI's job); live-Flute integration tests (would require real creds and
be flaky in CI).

## Success criteria

1. `cargo build --release` produces a single `flute-webhooks-mcp` binary.
2. `cargo test` green across all three layers; `cargo clippy --all-targets --no-deps -D warnings` clean; `cargo fmt --check` clean.
3. With `flute-webhook` installed and `flute-webhook auth login` already
   run for `uat`, configuring an MCP client (e.g. Claude Desktop) with
   `{ command: "flute-webhooks-mcp" }` exposes the eleven tools and
   `endpoints_list` returns real JSON.
4. With **no** credentials in the keychain, `endpoints_list` returns
   `isError: true` whose content includes `kind: "auth"` and a message
   telling the operator to run `flute-webhook auth login`. The server
   never panics.
5. `readme.md` documents the env vars (`FLUTE_PROFILE`,
   `FLUTE_WEBHOOK_BIN`, `FLUTE_MCP_TIMEOUT_SECS`, `FLUTE_MCP_DEBUG`),
   the eleven tool names, and a Claude Desktop config snippet.
