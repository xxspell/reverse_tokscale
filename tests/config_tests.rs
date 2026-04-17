use tokscale_activity_emulator::config::AppConfig;

#[test]
fn rejects_invalid_clients_mix_sum() {
    let yaml = r#"
clients_mix:
  claude_share: 0.9
  codex_share: 0.2
client_mode: both
daily_target:
  min_tokens: 10
  max_tokens: 20
  hard_cap_tokens: 100
"#;

    assert!(AppConfig::from_yaml_str(yaml).is_err());
}

#[test]
fn rejects_min_gt_max_tokens() {
    let yaml = r#"
daily_target:
  min_tokens: 20
  max_tokens: 10
  hard_cap_tokens: 100
client_mode: both
clients_mix:
  claude_share: 0.5
  codex_share: 0.5
"#;

    assert!(AppConfig::from_yaml_str(yaml).is_err());
}

#[test]
fn accepts_valid_config() {
    let yaml = r#"
daily_target:
  min_tokens: 10
  max_tokens: 20
  hard_cap_tokens: 100
client_mode: both
clients_mix:
  claude_share: 0.6
  codex_share: 0.4
runtime:
  seed: 42
  state_path: ~/.claude/projects/emulator-state.json
  claude_output_path: ~/.claude/projects/claude-activity.jsonl
  codex_output_path: ~/.codex/sessions/codex-activity.jsonl
"#;

    let cfg = AppConfig::from_yaml_str(yaml).expect("valid config must parse");
    assert_eq!(cfg.daily_target.min_tokens, 10);
    assert_eq!(cfg.daily_target.max_tokens, 20);
    assert_eq!(cfg.daily_target.hard_cap_tokens, 100);
    assert_eq!(cfg.runtime.seed, 42);
    assert!((cfg.clients_mix.claude_share - 0.6).abs() < 1e-9);
    assert!((cfg.clients_mix.codex_share - 0.4).abs() < 1e-9);
}

#[test]
fn rejects_config_without_runtime_block() {
    let yaml = r#"
daily_target:
  min_tokens: 10
  max_tokens: 20
  hard_cap_tokens: 100
client_mode: both
clients_mix:
  claude_share: 0.6
  codex_share: 0.4
"#;

    assert!(AppConfig::from_yaml_str(yaml).is_err());
}
