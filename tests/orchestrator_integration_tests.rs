use std::fs;

use serde_json::Value;

use chrono::NaiveDate;
use tempfile::tempdir;
use tokscale_activity_emulator::config::{AppConfig, ClientMode};
use tokscale_activity_emulator::orchestrator::{
    run_once_with_config_for_day, run_once_with_mode, run_once_with_paths,
};

fn count_lines(path: &std::path::Path) -> usize {
    if !path.exists() {
        return 0;
    }

    fs::read_to_string(path)
        .expect("read jsonl")
        .lines()
        .count()
}

fn client_outputs(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        root.join("claude-activity.jsonl"),
        root.join("codex-activity.jsonl"),
    )
}

fn parse_json_lines(path: &std::path::Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("read jsonl")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("parse json line"))
        .collect()
}

#[test]
fn second_run_same_completed_day_is_skip() {
    let dir = tempdir().expect("tempdir");
    let state_path = dir.path().join("emulator-state.json");
    let jsonl_path = dir.path().join("activity.jsonl");
    let day = NaiveDate::from_ymd_opt(2026, 4, 10).unwrap();

    let first = run_once_with_paths(day, &state_path, &jsonl_path, 20_000, 42).expect("first run");
    let lines_after_first = count_lines(&jsonl_path);

    let second =
        run_once_with_paths(day, &state_path, &jsonl_path, 20_000, 42).expect("second run");
    let lines_after_second = count_lines(&jsonl_path);

    assert_eq!(first.processed_days, 1);
    assert_eq!(first.skipped_days, 0);
    assert!(first.appended_lines > 0);

    assert_eq!(second.processed_days, 0);
    assert_eq!(second.skipped_days, 1);
    assert_eq!(second.appended_lines, 0);

    assert_eq!(lines_after_first, lines_after_second);
}

#[test]
fn missing_state_recovers_from_jsonl_and_resumes_from_next_day() {
    let dir = tempdir().expect("tempdir");
    let state_path = dir.path().join("emulator-state.json");
    let jsonl_path = dir.path().join("activity.jsonl");
    let day1 = NaiveDate::from_ymd_opt(2026, 4, 10).unwrap();
    let day2 = NaiveDate::from_ymd_opt(2026, 4, 11).unwrap();

    run_once_with_paths(day1, &state_path, &jsonl_path, 18_000, 99).expect("initial run");
    let lines_after_day1 = count_lines(&jsonl_path);

    fs::remove_file(&state_path).expect("remove state file");

    let resumed =
        run_once_with_paths(day2, &state_path, &jsonl_path, 18_000, 99).expect("resumed run");
    let lines_after_day2 = count_lines(&jsonl_path);

    assert!(resumed.resumed_from_jsonl);
    assert_eq!(resumed.processed_days, 1);
    assert_eq!(resumed.skipped_days, 0);
    assert!(resumed.appended_lines > 0);
    assert!(lines_after_day2 > lines_after_day1);

    let state_raw = fs::read_to_string(&state_path).expect("state exists after resumed run");
    assert!(state_raw.contains("2026-04-11"));
}

#[test]
fn client_mode_matrix_generates_expected_outputs() {
    let day = NaiveDate::from_ymd_opt(2026, 4, 12).unwrap();
    let shares = (0.5, 0.5);

    // both => claude + codex files
    let both_dir = tempdir().expect("both tempdir");
    let both_state = both_dir.path().join("emulator-state.json");
    let both_out = both_dir.path().join("out");
    let both = run_once_with_mode(
        day,
        &both_state,
        &both_out,
        24_000,
        7,
        ClientMode::Both,
        shares.0,
        shares.1,
    )
    .expect("both run");
    let (both_claude, both_codex) = client_outputs(&both_out);
    assert_eq!(both.processed_days, 1);
    assert!(both.appended_lines > 0);
    assert!(both_claude.exists());
    assert!(both_codex.exists());
    assert!(count_lines(&both_claude) > 0);
    assert!(count_lines(&both_codex) > 0);

    // claude_only => only claude file
    let claude_dir = tempdir().expect("claude tempdir");
    let claude_state = claude_dir.path().join("emulator-state.json");
    let claude_out = claude_dir.path().join("out");
    let claude = run_once_with_mode(
        day,
        &claude_state,
        &claude_out,
        24_000,
        7,
        ClientMode::ClaudeOnly,
        1.0,
        0.0,
    )
    .expect("claude-only run");
    let (claude_file, codex_file) = client_outputs(&claude_out);
    assert_eq!(claude.processed_days, 1);
    assert!(claude.appended_lines > 0);
    assert!(claude_file.exists());
    assert!(!codex_file.exists());
    assert!(count_lines(&claude_file) > 0);

    // codex_only => only codex file
    let codex_dir = tempdir().expect("codex tempdir");
    let codex_state = codex_dir.path().join("emulator-state.json");
    let codex_out = codex_dir.path().join("out");
    let codex = run_once_with_mode(
        day,
        &codex_state,
        &codex_out,
        24_000,
        7,
        ClientMode::CodexOnly,
        0.0,
        1.0,
    )
    .expect("codex-only run");
    let (claude_file, codex_file) = client_outputs(&codex_out);
    assert_eq!(codex.processed_days, 1);
    assert!(codex.appended_lines > 0);
    assert!(!claude_file.exists());
    assert!(codex_file.exists());
    assert!(count_lines(&codex_file) > 0);
}

#[test]
fn generated_jsonl_matches_tokscale_expected_shapes() {
    let dir = tempdir().expect("tempdir");
    let state_path = dir.path().join("emulator-state.json");
    let out_root = dir.path().join("out");
    let day = NaiveDate::from_ymd_opt(2026, 2, 12).unwrap();

    let summary = run_once_with_mode(
        day,
        &state_path,
        &out_root,
        24_000,
        7,
        ClientMode::Both,
        0.5,
        0.5,
    )
    .expect("run emulator");
    assert_eq!(summary.processed_days, 1);

    let (claude_file, codex_file) = client_outputs(&out_root);
    let claude_lines = parse_json_lines(&claude_file);
    let codex_lines = parse_json_lines(&codex_file);

    assert!(!claude_lines.is_empty(), "claude output must not be empty");
    assert!(!codex_lines.is_empty(), "codex output must not be empty");

    let claude_has_assistant_usage = claude_lines.iter().any(|line| {
        line.get("type").and_then(Value::as_str) == Some("assistant")
            && line
                .get("message")
                .and_then(Value::as_object)
                .and_then(|msg| msg.get("model"))
                .is_some()
            && line
                .get("message")
                .and_then(Value::as_object)
                .and_then(|msg| msg.get("usage"))
                .is_some()
    });
    assert!(
        claude_has_assistant_usage,
        "expected at least one Claude assistant entry with message.model and message.usage"
    );

    let codex_has_event_msg_token_count = codex_lines.iter().any(|line| {
        line.get("type").and_then(Value::as_str) == Some("event_msg")
            && line
                .get("payload")
                .and_then(Value::as_object)
                .and_then(|payload| payload.get("type"))
                .and_then(Value::as_str)
                == Some("token_count")
            && line
                .get("payload")
                .and_then(Value::as_object)
                .and_then(|payload| payload.get("info"))
                .and_then(Value::as_object)
                .and_then(|info| info.get("last_token_usage"))
                .is_some()
    });
    assert!(
        codex_has_event_msg_token_count,
        "expected at least one Codex event_msg token_count with payload.info.last_token_usage"
    );
}

#[test]
fn repeated_runs_append_new_days_without_overwriting_old_days() {
    let dir = tempdir().expect("tempdir");
    let state_path = dir.path().join("emulator-state.json");
    let claude_path = dir.path().join("claude-activity.jsonl");
    let codex_path = dir.path().join("codex-activity.jsonl");

    let cfg_yaml = format!(
        r#"
daily_target:
  min_tokens: 12000
  max_tokens: 12000
  hard_cap_tokens: 12000
clients_mix:
  claude_share: 0.7
  codex_share: 0.3
client_mode: both
runtime:
  start_day: 2026-04-10
  seed: 42
  state_path: {}
  claude_output_path: {}
  codex_output_path: {}
"#,
        state_path.display(),
        claude_path.display(),
        codex_path.display()
    );

    let cfg = AppConfig::from_yaml_str(&cfg_yaml).expect("valid config");

    let day1 = NaiveDate::from_ymd_opt(2026, 4, 10).unwrap();
    let day2 = NaiveDate::from_ymd_opt(2026, 4, 11).unwrap();

    let first = run_once_with_config_for_day(day1, &cfg).expect("first run");
    assert_eq!(first.processed_days, 1);

    let first_claude = parse_json_lines(&claude_path);
    let first_codex = parse_json_lines(&codex_path);

    assert!(first_claude.iter().all(|line| {
        line.get("day").and_then(Value::as_str) == Some("2026-04-10")
    }));
    assert!(first_codex.iter().all(|line| {
        line.get("day").and_then(Value::as_str) == Some("2026-04-10")
    }));

    let second = run_once_with_config_for_day(day2, &cfg).expect("second run");
    assert_eq!(second.processed_days, 1);

    let second_claude = parse_json_lines(&claude_path);
    let second_codex = parse_json_lines(&codex_path);

    assert!(second_claude.len() > first_claude.len());
    assert!(second_codex.len() > first_codex.len());

    let claude_days: std::collections::BTreeSet<String> = second_claude
        .iter()
        .filter_map(|line| line.get("day").and_then(Value::as_str))
        .map(|s| s.to_string())
        .collect();
    let codex_days: std::collections::BTreeSet<String> = second_codex
        .iter()
        .filter_map(|line| line.get("day").and_then(Value::as_str))
        .map(|s| s.to_string())
        .collect();

    assert_eq!(
        claude_days,
        ["2026-04-10".to_string(), "2026-04-11".to_string()]
            .into_iter()
            .collect()
    );
    assert_eq!(
        codex_days,
        ["2026-04-10".to_string(), "2026-04-11".to_string()]
            .into_iter()
            .collect()
    );
}

#[test]
fn submit_payload_after_second_run_contains_both_days() {
    let dir = tempdir().expect("tempdir");
    let state_path = dir.path().join("emulator-state.json");
    let claude_path = dir.path().join("claude-activity.jsonl");
    let codex_path = dir.path().join("codex-activity.jsonl");
    let submit_payload_path = dir.path().join("submit-payload.json");

    let cfg_yaml = format!(
        r#"
daily_target:
  min_tokens: 10000
  max_tokens: 10000
  hard_cap_tokens: 10000
clients_mix:
  claude_share: 0.7
  codex_share: 0.3
client_mode: both
runtime:
  start_day: 2026-04-10
  seed: 42
  state_path: {}
  claude_output_path: {}
  codex_output_path: {}
  submit_payload_output_path: {}
"#,
        state_path.display(),
        claude_path.display(),
        codex_path.display(),
        submit_payload_path.display()
    );

    let cfg = AppConfig::from_yaml_str(&cfg_yaml).expect("config with submit payload path");

    run_once_with_config_for_day(NaiveDate::from_ymd_opt(2026, 4, 10).unwrap(), &cfg)
        .expect("run day 1");
    run_once_with_config_for_day(NaiveDate::from_ymd_opt(2026, 4, 11).unwrap(), &cfg)
        .expect("run day 2");

    let payload: Value = serde_json::from_str(
        &fs::read_to_string(&submit_payload_path).expect("read submit payload"),
    )
    .expect("valid submit payload json");

    let contributions = payload
        .get("contributions")
        .and_then(Value::as_array)
        .expect("contributions array");

    let days: std::collections::BTreeSet<String> = contributions
        .iter()
        .filter_map(|c| c.get("date").and_then(Value::as_str))
        .map(|s| s.to_string())
        .collect();

    assert_eq!(
        days,
        ["2026-04-10".to_string(), "2026-04-11".to_string()]
            .into_iter()
            .collect()
    );
}

#[test]
fn submit_payload_json_is_emitted_and_contains_expected_fields() {
    let dir = tempdir().expect("tempdir");
    let state_path = dir.path().join("emulator-state.json");
    let claude_path = dir.path().join("claude-activity.jsonl");
    let codex_path = dir.path().join("codex-activity.jsonl");
    let submit_payload_path = dir.path().join("submit-payload.json");

    let cfg_yaml = format!(
        r#"
daily_target:
  min_tokens: 10000
  max_tokens: 10000
  hard_cap_tokens: 10000
clients_mix:
  claude_share: 0.7
  codex_share: 0.3
client_mode: both
runtime:
  start_day: 2026-04-12
  seed: 42
  state_path: {}
  claude_output_path: {}
  codex_output_path: {}
  submit_payload_output_path: {}
"#,
        state_path.display(),
        claude_path.display(),
        codex_path.display(),
        submit_payload_path.display()
    );

    let cfg = AppConfig::from_yaml_str(&cfg_yaml).expect("config with submit payload path");
    let day = NaiveDate::from_ymd_opt(2026, 4, 12).unwrap();

    run_once_with_config_for_day(day, &cfg).expect("run emulator");

    let payload: Value = serde_json::from_str(
        &fs::read_to_string(&submit_payload_path).expect("read submit payload"),
    )
    .expect("valid submit payload json");

    assert!(payload.get("meta").is_some());
    assert!(payload.get("summary").is_some());
    assert!(payload.get("years").is_some());
    assert!(payload.get("contributions").is_some());

    let contributions = payload
        .get("contributions")
        .and_then(Value::as_array)
        .expect("contributions array");
    assert!(!contributions.is_empty());

    let meta = payload.get("meta").and_then(Value::as_object).expect("meta object");
    assert!(meta.get("dateRange").is_some());

    let summary = payload
        .get("summary")
        .and_then(Value::as_object)
        .expect("summary object");
    assert!(summary.get("totalCost").is_some());
    assert!(
        summary
            .get("totalCost")
            .and_then(Value::as_f64)
            .is_some_and(|v| v > 0.0)
    );
    assert!(summary.get("totalDays").is_some());
    assert!(summary.get("activeDays").is_some());
    assert!(summary.get("averagePerDay").is_some());
    assert!(summary.get("maxCostInSingleDay").is_some());
    assert!(summary.get("clients").and_then(Value::as_array).is_some());
    assert!(summary.get("models").and_then(Value::as_array).is_some());

    let first = &contributions[0];
    let totals = first
        .get("totals")
        .and_then(Value::as_object)
        .expect("totals object");
    assert!(totals.get("tokens").is_some());
    assert!(totals.get("cost").is_some());
    assert!(totals.get("cost").and_then(Value::as_f64).is_some_and(|v| v > 0.0));
    assert!(totals.get("messages").is_some());

    assert!(first.get("intensity").is_some());

    let token_breakdown = first
        .get("tokenBreakdown")
        .and_then(Value::as_object)
        .expect("tokenBreakdown object");
    assert!(token_breakdown.get("input").is_some());
    assert!(token_breakdown.get("output").is_some());
    assert!(token_breakdown.get("cacheRead").is_some());
    assert!(token_breakdown.get("cacheWrite").is_some());
    assert!(token_breakdown.get("reasoning").is_some());

    let clients = first
        .get("clients")
        .and_then(Value::as_array)
        .expect("clients array");
    let claude_client = clients
        .iter()
        .find(|c| c.get("client").and_then(Value::as_str) == Some("claude"))
        .expect("claude client entry");
    assert_eq!(
        claude_client.get("modelId").and_then(Value::as_str),
        Some("claude-opus-4-7-20250805")
    );
    assert!(
        claude_client
            .get("cost")
            .and_then(Value::as_f64)
            .is_some_and(|v| v > 0.0)
    );
}
