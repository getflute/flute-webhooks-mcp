# flute-webhooks-mcp

An MCP (Model Context Protocol) server that lets an AI agent drive Aurora's `flute-webhooks` CLI.

## How it works

The server spawns `flute-webhooks --output json …` once per tool call, parses the structured stdout, and surfaces both successes and the upstream CLI's `{kind, message, status?, correlation_id?}` error envelope through MCP. Auth lives in the OS keychain on the operator's machine; this server never touches credentials directly.

## Install

Pick whichever installer matches your platform. Each drops a `flute-webhooks-mcp` binary in a directory your *shell* likely has on `PATH` — your MCP client probably does not (see [Binary paths](#binary-paths)).

```bash
# macOS / Linux (curl + sh)
curl -LsSf https://github.com/getflute/flute-webhooks-mcp/releases/latest/download/flute-webhooks-mcp-installer.sh | sh

# macOS / Linux (Homebrew)
brew install getflute/flute-webhooks-mcp/flute-webhooks-mcp

# Windows (PowerShell)
irm https://github.com/getflute/flute-webhooks-mcp/releases/latest/download/flute-webhooks-mcp-installer.ps1 | iex
```

That `brew install` line taps [`getflute/homebrew-flute-webhooks-mcp`](https://github.com/getflute/homebrew-flute-webhooks-mcp) — a separate repository this one pushes its formula to on every release. (Homebrew always rewrites `user/repo` as `github.com/user/homebrew-repo`; a project repo can never serve as its own tap.) The formula covers Apple Silicon macOS and x86_64 Linux — the two Unix targets released here. On Intel macOS, use the shell installer.

Or, to build from source:

```bash
cargo install --path .
```

Prereq: install **`flute-webhooks` v0.7.1 or newer** (see [getflute/flute-webhooks-cli](https://github.com/getflute/flute-webhooks-cli) and [Upstream CLI version](#upstream-cli-version)) and run `flute-webhooks auth login` once per profile you'll use. Note where both binaries land — you need their absolute paths to configure a client (see [Binary paths](#binary-paths)).

## Run

```bash
flute-webhooks-mcp        # talks JSON-RPC over stdio
```

Start one server instance per environment (`sandbox` vs `production`) — the profile is **pinned at startup**.

Two flags mirror the env vars, for a client that can set arguments more easily than an environment: `--binary <path>` (same as `FLUTE_WEBHOOKS_BIN`) and `--profile <sandbox|production>` (same as `FLUTE_PROFILE`). The flag wins over the env var.

## Environment variables

| Variable | Default | Purpose |
|---|---|---|
| `FLUTE_PROFILE` | `sandbox` | `sandbox` or `production` (alias `prod`). Pinned at startup. |
| `FLUTE_WEBHOOKS_BIN` | *(unset — falls back to a `PATH` lookup that usually fails under an MCP client; set it)* | Absolute path to the `flute-webhooks` binary. See [Binary paths](#binary-paths). |
| `FLUTE_MCP_TIMEOUT_SECS` | `30` | Per-call timeout for the child process. |
| `FLUTE_MCP_DEBUG` | unset | When set to any non-empty value — including `0` and `false` — route `flute-webhooks` stderr to this server's tracing layer. Unset it to turn it off. |
| `RUST_LOG` | `info` | Standard `tracing` filter. Logs go to *stderr* only. |

## Binary paths

**Assume neither binary is on the client's `PATH`, and configure both by absolute path.**

Your MCP client spawns `flute-webhooks-mcp` directly as a child process — it does not run your login shell first. A client started from the macOS Dock, Windows Explorer, or an IDE inherits a minimal `PATH` (often just `/usr/bin:/bin:/usr/sbin:/sbin`) containing none of the directories your `.zshrc`/`.bashrc` or the installers add: `/usr/local/bin`, `/opt/homebrew/bin`, `~/.local/bin`. That `flute-webhooks-mcp` runs fine when *you* type it in a terminal proves nothing here — that `PATH` is your shell's, not the client's.

Two separate lookups depend on this, and each fails differently:

- **`command`** — the path the client uses to launch this server. If it can't be resolved, the server never starts and the client reports it as failed or disconnected, with nothing in this server's logs (there are none yet).
- **`FLUTE_WEBHOOKS_BIN`** — where this server finds the `flute-webhooks` CLI. Left unset, it falls back to a `PATH` lookup for `flute-webhooks`; when that misses, the server prints ``could not find `flute-webhooks` on PATH`` to stderr and exits 2 **before serving a single request**, so this too surfaces as a dead server rather than a tool error.

Get the real paths from your own shell:

```bash
# macOS / Linux
command -v flute-webhooks
command -v flute-webhooks-mcp
```

```powershell
# Windows (PowerShell)
(Get-Command flute-webhooks).Source
(Get-Command flute-webhooks-mcp).Source
```

If those come up empty, the binary isn't installed for this user — install it first (above) rather than guessing a path. For reference, the installers put `flute-webhooks-mcp` in:

| Installed via | Location |
|---|---|
| shell / PowerShell installer, `cargo install` | `~/.cargo/bin` (Windows: `%USERPROFILE%\.cargo\bin`) |
| Homebrew, Apple Silicon macOS | `/opt/homebrew/bin` |
| Linuxbrew, x86_64 | `/home/linuxbrew/.linuxbrew/bin` |

`FLUTE_WEBHOOKS_BIN` must name the executable itself, not the directory holding it. Point it at a missing path or a directory and the server exits 2 at startup with ``configuration error: FLUTE_WEBHOOKS_BIN=`…` does not exist or is not executable`` — checked at launch on purpose, so a bad path shows up immediately instead of on the first tool call. The check is existence-only despite that wording, so a file without the executable bit gets past startup and fails on the first tool call with `kind:"spawn"` — as does a binary that disappears after startup.

Neither config format expands `~` or `$HOME` — write the path out in full. On Windows, escape the backslashes in JSON (`"C:\\Program Files\\flute-webhooks\\flute-webhooks.exe"`) or use a TOML literal string (`'C:\Program Files\flute-webhooks\flute-webhooks.exe'`), and include the `.exe`.

## Claude Desktop config

Replace both `/path/to/…` placeholders with the absolute paths you found above.

```jsonc
{
  "mcpServers": {
    "flute-webhooks-sandbox": {
      "command": "/path/to/mcp/flute-webhooks-mcp",
      "env": {
        "FLUTE_PROFILE": "sandbox",
        "FLUTE_WEBHOOKS_BIN": "/path/to/cli/flute-webhooks"
      }
    },
    "flute-webhooks-prod": {
      "command": "/path/to/mcp/flute-webhooks-mcp",
      "env": {
        "FLUTE_PROFILE": "production",
        "FLUTE_WEBHOOKS_BIN": "/path/to/cli/flute-webhooks"
      }
    }
  }
}
```

If a server shows as failed, check the client's MCP logs for this server's stderr — on macOS, `~/Library/Logs/Claude/mcp-server-flute-webhooks-sandbox.log`. A `could not find flute-webhooks on PATH` line there means `FLUTE_WEBHOOKS_BIN` is unset or wrong; no log file at all usually means `command` itself didn't resolve.

## Codex app config

Codex stores MCP servers in `~/.codex/config.toml`. The Codex app, CLI, and IDE extension share this configuration — so even if you only ever launch `codex` from a shell that has both binaries on `PATH`, set the absolute paths anyway or the same config breaks under the app and the extension.

```toml
[mcp_servers.flute-webhooks-sandbox]
command = "/path/to/mcp/flute-webhooks-mcp"
env = { FLUTE_PROFILE = "sandbox", FLUTE_WEBHOOKS_BIN = "/path/to/cli/flute-webhooks" }

[mcp_servers.flute-webhooks-prod]
command = "/path/to/mcp/flute-webhooks-mcp"
env = { FLUTE_PROFILE = "production", FLUTE_WEBHOOKS_BIN = "/path/to/cli/flute-webhooks" }
```

## Tool inventory

| Tool | Idempotent? |
|---|---|
| `endpoints_list` | yes |
| `endpoints_get` | yes |
| `endpoints_create` | **no** — duplicates create a second endpoint |
| `endpoints_update` | yes (PATCH, JSON Merge Patch — omitted fields are left unchanged) |
| `endpoints_delete` | yes (second call returns 404) |
| `endpoints_ping` | yes |
| `event_types_list` | yes |
| `deliveries_list` | yes |
| `deliveries_get` | yes |
| `deliveries_retry` | **no** — each call schedules another retry |
| `auth_status` | yes |

Excluded by design: the upstream `tui`, `auth login` (interactive), `listen` (long-running, no JSON), and `update` (operator-only).

Tool results are the upstream CLI's JSON passed through verbatim, so the shapes an agent sees come from `flute-webhooks`, not from this server. The tool descriptions document the v0.7.1 contract: `endpoints_list` and `deliveries_list` return `{items, pageInfo}`, `endpoints_update` is a merge PATCH, `deliveries_retry` returns a full delivery log, and `auth_status` shells out to `auth keys`.

Older CLIs are **not** supported — see [Upstream CLI version](#upstream-cli-version).

## Upstream CLI version

**Minimum: `flute-webhooks` v0.7.1.** Developed and tested against v0.7.4. Supporting older CLIs is deliberately out of scope — this server tracks the current upstream contract rather than bridging versions.

Two upstream changes set that floor:

| Needed for | Landed in | What this server relies on |
|---|---|---|
| Every tool's result shape | v0.7.0 | The Flute v2 spec pass: `{items, pageInfo}` list envelopes, `PATCH` on endpoint update, renamed wire fields, and `deliveries retry` returning a full delivery log |
| `auth_status` | v0.7.1 | The `auth keys` subcommand (`auth token` was its former name) |

Check yours with `flute-webhooks --version`.

## Errors

Every tool returns either a success result or `isError: true` with a structured JSON content item containing at minimum a `kind` field — one of `api`, `transport`, `auth`, `decode`, `client`, `spawn`, `timeout`, `bad_output`. `api` errors also carry `status` and (where the server provided one) `correlation_id`.

For an agent: branch on `kind` first. `transport` and `api` with status ∈ {500,502,503,504} are safe to retry with backoff. `auth` means run `flute-webhooks auth login` on the operator's machine.

Startup failures never reach this layer: a bad or missing `flute-webhooks` path makes the process exit 2 with a plain stderr line, so the client sees a server that won't start. See [Binary paths](#binary-paths).

## License

MIT.
