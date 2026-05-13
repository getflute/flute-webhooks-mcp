use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use tempfile::TempDir;

fn write_fake_flute(dir: &TempDir, stdout: &str, exit_code: i32) -> std::path::PathBuf {
    let path = dir.path().join("flute-webhook");
    let script = format!(
        "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{}'\nexit {}\n",
        stdout.replace('\'', "'\\''"),
        exit_code,
    );
    fs::write(&path, script).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

fn jsonrpc(id: u64, method: &str, params: Value) -> String {
    let frame = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    format!("{}\n", serde_json::to_string(&frame).unwrap())
}

fn jsonrpc_notify(method: &str, params: Value) -> String {
    // JSON-RPC notifications have no "id" field — servers must not respond to them.
    let frame = json!({"jsonrpc":"2.0","method":method,"params":params});
    format!("{}\n", serde_json::to_string(&frame).unwrap())
}

fn read_one_frame(reader: &mut BufReader<impl std::io::Read>) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read frame");
    serde_json::from_str(line.trim()).expect("parse frame")
}

#[test]
fn lists_tools_and_calls_endpoints_list_through_stdio() {
    let dir = TempDir::new().unwrap();
    let fake = write_fake_flute(&dir, r#"{"data":[{"id":"e1"}]}"#, 0);

    let bin = assert_cmd::cargo::cargo_bin("flute-webhooks-mcp");
    let mut child = Command::new(bin)
        .env("FLUTE_WEBHOOK_BIN", &fake)
        .env("FLUTE_PROFILE", "uat")
        .env("RUST_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn server");

    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    // 1. initialize
    stdin
        .write_all(
            jsonrpc(
                1,
                "initialize",
                json!({
                    "protocolVersion":"2025-03-26",
                    "capabilities":{},
                    "clientInfo":{"name":"e2e","version":"0"}
                }),
            )
            .as_bytes(),
        )
        .unwrap();
    let init = read_one_frame(&mut reader);
    assert_eq!(init["id"], 1);
    assert!(init["result"].is_object());

    // 2. notifications/initialized (one-way, no response expected)
    stdin
        .write_all(jsonrpc_notify("notifications/initialized", json!({})).as_bytes())
        .unwrap();

    // 3. tools/list — assert our tools are exposed
    stdin
        .write_all(jsonrpc(2, "tools/list", json!({})).as_bytes())
        .unwrap();
    let listed = read_one_frame(&mut reader);
    let names: Vec<String> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    for expected in &[
        "endpoints_list",
        "endpoints_get",
        "endpoints_create",
        "endpoints_update",
        "endpoints_delete",
        "endpoints_ping",
        "event_types_list",
        "deliveries_list",
        "deliveries_get",
        "deliveries_retry",
        "auth_status",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "missing tool {expected} in {names:?}"
        );
    }

    // 4. tools/call endpoints_list — round-trip the payload
    stdin
        .write_all(
            jsonrpc(
                3,
                "tools/call",
                json!({
                    "name":"endpoints_list","arguments":{}
                }),
            )
            .as_bytes(),
        )
        .unwrap();
    let called = read_one_frame(&mut reader);
    assert_eq!(called["id"], 3);
    let content = &called["result"]["content"][0];
    let text = content["text"].as_str().unwrap();
    assert!(text.contains("\"e1\""), "got {text}");

    // Cleanup
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn auth_error_surfaces_as_is_error() {
    let dir = TempDir::new().unwrap();
    let fake = write_fake_flute(
        &dir,
        r#"{"kind":"auth","message":"no credentials for [uat]"}"#,
        1,
    );
    let bin = assert_cmd::cargo::cargo_bin("flute-webhooks-mcp");
    let mut child = Command::new(bin)
        .env("FLUTE_WEBHOOK_BIN", &fake)
        .env("FLUTE_PROFILE", "uat")
        .env("RUST_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    stdin
        .write_all(
            jsonrpc(
                1,
                "initialize",
                json!({
                    "protocolVersion":"2025-03-26","capabilities":{},
                    "clientInfo":{"name":"e2e","version":"0"}
                }),
            )
            .as_bytes(),
        )
        .unwrap();
    let _ = read_one_frame(&mut reader);
    stdin
        .write_all(jsonrpc_notify("notifications/initialized", json!({})).as_bytes())
        .unwrap();

    stdin
        .write_all(
            jsonrpc(
                2,
                "tools/call",
                json!({
                    "name":"endpoints_list","arguments":{}
                }),
            )
            .as_bytes(),
        )
        .unwrap();
    let called = read_one_frame(&mut reader);
    assert_eq!(called["result"]["isError"], json!(true));
    let text = called["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"auth\""), "got {text}");
    assert!(text.contains("auth login"), "got {text}");

    let _ = child.kill();
    let _ = child.wait();
}
