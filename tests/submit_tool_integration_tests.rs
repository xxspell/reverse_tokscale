use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use chrono::Local;
use serde_json::Value;
use tempfile::tempdir;

fn write_submit_state(path: &Path, day: &str) {
    fs::write(path, format!("{{\"last_submitted_day\":\"{}\"}}", day)).expect("write state");
}

fn minimal_cfg_yaml(start_day: &str, payload: &Path, submit_state: &Path) -> String {
    format!(
        r#"
daily_target:
  min_tokens: 10000
  max_tokens: 12000
  hard_cap_tokens: 20000
clients_mix:
  claude_share: 1.0
  codex_share: 0.0
client_mode: claude_only
runtime:
  start_day: {start_day}
  seed: 42
  submit_payload_output_path: {}
  submit_state_path: {}
"#,
        payload.display(),
        submit_state.display()
    )
}

fn spawn_submit_server() -> (SocketAddr, Arc<AtomicUsize>, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let addr = listener.local_addr().expect("local addr");
    let hits = Arc::new(AtomicUsize::new(0));
    let payloads = Arc::new(Mutex::new(Vec::<Value>::new()));

    let hits_clone = Arc::clone(&hits);
    let payloads_clone = Arc::clone(&payloads);

    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };

            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = match stream.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }

            let header_end = buf
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|i| i + 4)
                .expect("headers end");
            let headers = String::from_utf8_lossy(&buf[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|l| {
                    let lower = l.to_ascii_lowercase();
                    lower
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);

            let mut body = buf[header_end..].to_vec();
            while body.len() < content_length {
                let n = match stream.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };
                body.extend_from_slice(&tmp[..n]);
            }

            if headers.starts_with("POST /api/submit ") {
                if let Ok(v) = serde_json::from_slice::<Value>(&body) {
                    payloads_clone.lock().expect("payload lock").push(v);
                }
                let idx = hits_clone.fetch_add(1, Ordering::SeqCst) + 1;
                let body = format!("{{\"success\":true,\"mode\":\"merge\",\"n\":{idx}}}");
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            } else {
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        }
    });

    (addr, hits, payloads)
}

fn run_submit(args: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("--bin")
        .arg("tokscale-submit")
        .arg("--manifest-path")
        .arg("/Users/xxspell/Code/reverse-tokscale/Cargo.toml")
        .arg("--")
        .args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("run tokscale-submit")
}

#[test]
fn run_generates_until_today_from_start_day() {
    let dir = tempdir().expect("tempdir");
    let state = dir.path().join("submit-state.json");
    let payload = dir.path().join("payload.json");
    let cfg = dir.path().join("cfg.yaml");

    let (addr, hits, _) = spawn_submit_server();
    let today = Local::now().date_naive();
    let start = today - chrono::Days::new(2);

    fs::write(&cfg, minimal_cfg_yaml(&start.format("%Y-%m-%d").to_string(), &payload, &state))
        .expect("write cfg");

    let out = run_submit(
        &["run", cfg.to_str().unwrap()],
        &[("TOKSCALE_API_URL", &format!("http://{}", addr)), ("TOKSCALE_TOKEN", "test-token")],
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let payload_json: Value = serde_json::from_str(&fs::read_to_string(&payload).expect("payload")).unwrap();
    let contributions = payload_json
        .get("contributions")
        .and_then(Value::as_array)
        .expect("contributions");
    assert!(contributions.len() >= 3);
}

#[test]
fn run_submits_without_creating_client_jsonl_files() {
    let dir = tempdir().expect("tempdir");
    let state = dir.path().join("submit-state.json");
    let payload = dir.path().join("payload.json");
    let cfg = dir.path().join("cfg.yaml");
    let claude = dir.path().join("claude.jsonl");
    let codex = dir.path().join("codex.jsonl");

    let (addr, hits, _) = spawn_submit_server();
    let start = (Local::now().date_naive() - chrono::Days::new(2)).format("%Y-%m-%d").to_string();
    fs::write(&cfg, minimal_cfg_yaml(&start, &payload, &state)).expect("write cfg");

    let out = run_submit(
        &["run", cfg.to_str().unwrap(), "--shard-days", "2"],
        &[("TOKSCALE_API_URL", &format!("http://{}", addr)), ("TOKSCALE_TOKEN", "test-token")],
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    assert!(hits.load(Ordering::SeqCst) > 0);
    assert!(!claude.exists());
    assert!(!codex.exists());
}

#[test]
fn run_submits_sharded_and_updates_submit_state() {
    let dir = tempdir().expect("tempdir");
    let state = dir.path().join("submit-state.json");
    let payload = dir.path().join("payload.json");
    let cfg = dir.path().join("cfg.yaml");

    let (addr, hits, payloads) = spawn_submit_server();
    let today = Local::now().date_naive();
    let start = today - chrono::Days::new(5);
    fs::write(&cfg, minimal_cfg_yaml(&start.format("%Y-%m-%d").to_string(), &payload, &state))
        .expect("write cfg");

    let out = run_submit(
        &["run", cfg.to_str().unwrap(), "--shard-days", "2"],
        &[("TOKSCALE_API_URL", &format!("http://{}", addr)), ("TOKSCALE_TOKEN", "test-token")],
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let total_days = today.signed_duration_since(start).num_days() + 1;
    let expected_hits = ((total_days + 1) / 2) as usize;
    assert_eq!(hits.load(Ordering::SeqCst), expected_hits);

    let last_state: Value = serde_json::from_str(&fs::read_to_string(&state).expect("state")).unwrap();
    let today_str = today.format("%Y-%m-%d").to_string();
    assert_eq!(last_state.get("last_submitted_day").and_then(Value::as_str), Some(today_str.as_str()));

    assert_eq!(payloads.lock().expect("payload lock").len(), expected_hits);
}

#[test]
fn second_run_skips_already_submitted_range() {
    let dir = tempdir().expect("tempdir");
    let state = dir.path().join("submit-state.json");
    let payload = dir.path().join("payload.json");
    let cfg = dir.path().join("cfg.yaml");

    let (addr, hits, _) = spawn_submit_server();
    let start = (Local::now().date_naive() - chrono::Days::new(1)).format("%Y-%m-%d").to_string();
    fs::write(&cfg, minimal_cfg_yaml(&start, &payload, &state)).expect("write cfg");

    let first = run_submit(
        &["run", cfg.to_str().unwrap(), "--shard-days", "10"],
        &[("TOKSCALE_API_URL", &format!("http://{}", addr)), ("TOKSCALE_TOKEN", "test-token")],
    );
    assert!(first.status.success());

    let second = run_submit(
        &["run", cfg.to_str().unwrap(), "--shard-days", "10"],
        &[("TOKSCALE_API_URL", &format!("http://{}", addr)), ("TOKSCALE_TOKEN", "test-token")],
    );
    assert!(second.status.success());

    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[test]
fn reset_soft_replays_same_range() {
    let dir = tempdir().expect("tempdir");
    let state = dir.path().join("submit-state.json");
    let payload = dir.path().join("payload.json");
    let cfg = dir.path().join("cfg.yaml");

    let (addr, hits, _) = spawn_submit_server();
    let start = (Local::now().date_naive() - chrono::Days::new(1)).format("%Y-%m-%d").to_string();
    fs::write(&cfg, minimal_cfg_yaml(&start, &payload, &state)).expect("write cfg");

    let run1 = run_submit(
        &["run", cfg.to_str().unwrap()],
        &[("TOKSCALE_API_URL", &format!("http://{}", addr)), ("TOKSCALE_TOKEN", "test-token")],
    );
    assert!(run1.status.success());

    let reset = run_submit(&["reset", "--soft", cfg.to_str().unwrap()], &[]);
    assert!(reset.status.success());

    let run2 = run_submit(
        &["run", cfg.to_str().unwrap()],
        &[("TOKSCALE_API_URL", &format!("http://{}", addr)), ("TOKSCALE_TOKEN", "test-token")],
    );
    assert!(run2.status.success());

    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[test]
fn reset_hard_clears_submit_state_and_payload_cache() {
    let dir = tempdir().expect("tempdir");
    let state = dir.path().join("submit-state.json");
    let payload = dir.path().join("payload.json");
    let cfg = dir.path().join("cfg.yaml");

    write_submit_state(&state, "2026-04-10");
    fs::write(&payload, "{\"cached\":true}").expect("seed payload");
    fs::write(&cfg, minimal_cfg_yaml("2026-04-10", &payload, &state)).expect("write cfg");

    let reset = run_submit(&["reset", "--hard", cfg.to_str().unwrap()], &[]);
    assert!(reset.status.success());

    assert!(!state.exists());
    assert!(!payload.exists());
}
