use std::fs;
use std::path::Path;

use anyhow::{bail, Result};
use chrono::{Local, NaiveDate, TimeZone, Utc};
use serde_json::json;

use crate::allocator::split_daily_tokens;
use crate::claude_writer::append_jsonl_line as append_claude_jsonl_line;
use crate::codex_writer::append_jsonl_line as append_codex_jsonl_line;
use crate::config::{AppConfig, ClientMode};
use crate::humanizer::build_day_plan;
use crate::resume::{recover_state_from_jsonl, recover_state_from_jsonl_pair};
use crate::state::{checkpoint_state_atomic, load_state, EmulatorState};
use crate::submit_payload::write_submit_payload;
use crate::timeline::resolve_days;
use crate::validator::validate_events;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunSummary {
    pub processed_days: usize,
    pub skipped_days: usize,
    pub appended_lines: usize,
    pub last_generated_day: Option<NaiveDate>,
    pub resumed_from_jsonl: bool,
}

pub fn run_once() -> Result<()> {
    bail!("run_once requires explicit config path; use run_once_with_config_path")
}

pub fn run_once_with_config_path(config_path: &Path) -> Result<RunSummary> {
    let cfg = AppConfig::from_yaml_file(config_path)?;
    run_once_with_config_for_day(today_local(), &cfg)
}

pub fn run_once_with_config_for_day(day: NaiveDate, cfg: &AppConfig) -> Result<RunSummary> {
    let state_path = cfg.resolved_state_path()?;
    let claude_path = cfg.resolved_claude_output_path()?;
    let codex_path = cfg.resolved_codex_output_path()?;

    let summary = run_once_with_client_paths(
        day,
        &state_path,
        &claude_path,
        &codex_path,
        cfg.daily_tokens(),
        cfg.runtime.seed,
        cfg.client_mode,
        cfg.clients_mix.claude_share,
        cfg.clients_mix.codex_share,
        cfg.runtime.start_day,
    )?;

    if let Some(payload_path) = cfg.resolved_submit_payload_output_path()? {
        write_submit_payload(&payload_path, &claude_path, &codex_path)?;
    }

    Ok(summary)
}

pub fn run_once_with_paths(
    day: NaiveDate,
    state_path: &Path,
    jsonl_path: &Path,
    daily_tokens: u64,
    seed: u64,
) -> Result<RunSummary> {
    if let Some(parent) = jsonl_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut summary = RunSummary::default();

    let mut state = if let Some(state) = load_state(state_path)? {
        state
    } else {
        summary.resumed_from_jsonl = true;
        recover_state_from_jsonl(jsonl_path)?
    };

    let last_completed = state.last_generated_day.as_deref().and_then(parse_day);
    let days = resolve_days(day, last_completed, day);

    if days.is_empty() {
        summary.skipped_days = 1;
        summary.last_generated_day = last_completed;
        return Ok(summary);
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(jsonl_path)?;

    for target_day in days {
        let plan = build_day_plan(target_day, daily_tokens, seed);
        validate_events(&plan.events)?;

        for event in &plan.events {
            let line = claude_assistant_line(target_day, event)?;
            append_claude_jsonl_line(&mut file, &line)?;
            summary.appended_lines += 1;
        }

        state.last_generated_day = Some(target_day.format("%Y-%m-%d").to_string());
        checkpoint_state_atomic(state_path, &state)?;

        summary.processed_days += 1;
        summary.last_generated_day = Some(target_day);
    }

    Ok(summary)
}

#[allow(clippy::too_many_arguments)]
pub fn run_once_with_mode(
    day: NaiveDate,
    state_path: &Path,
    output_root: &Path,
    daily_tokens: u64,
    seed: u64,
    client_mode: ClientMode,
    claude_share: f64,
    codex_share: f64,
) -> Result<RunSummary> {
    fs::create_dir_all(output_root)?;

    let claude_path = output_root.join("claude-activity.jsonl");
    let codex_path = output_root.join("codex-activity.jsonl");

    run_once_with_client_paths(
        day,
        state_path,
        &claude_path,
        &codex_path,
        daily_tokens,
        seed,
        client_mode,
        claude_share,
        codex_share,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_once_with_client_paths(
    day: NaiveDate,
    state_path: &Path,
    claude_path: &Path,
    codex_path: &Path,
    daily_tokens: u64,
    seed: u64,
    client_mode: ClientMode,
    claude_share: f64,
    codex_share: f64,
    start_day: Option<NaiveDate>,
) -> Result<RunSummary> {
    if let Some(parent) = claude_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = codex_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut summary = RunSummary::default();

    let mut state = if let Some(state) = load_state(state_path)? {
        state
    } else {
        summary.resumed_from_jsonl = true;
        recover_state_from_jsonl_pair(claude_path, codex_path)?
    };

    let last_completed = state.last_generated_day.as_deref().and_then(parse_day);
    let start = start_day.unwrap_or(day);
    let days = resolve_days(start, last_completed, day);

    if days.is_empty() {
        summary.skipped_days = 1;
        summary.last_generated_day = last_completed;
        return Ok(summary);
    }

    for target_day in days {
        let (claude_tokens, codex_tokens) =
            split_daily_tokens(daily_tokens, claude_share, codex_share, client_mode);
        let claude_plan =
            (claude_tokens > 0).then(|| build_day_plan(target_day, claude_tokens, seed ^ 0xC1A0DE));
        let codex_plan =
            (codex_tokens > 0).then(|| build_day_plan(target_day, codex_tokens, seed ^ 0xC0D300));

        if let Some(plan) = &claude_plan {
            validate_events(&plan.events)?;
        }
        if let Some(plan) = &codex_plan {
            validate_events(&plan.events)?;
        }

        if let Some(plan) = &claude_plan {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(claude_path)?;

            for event in &plan.events {
                let line = claude_assistant_line(target_day, event)?;
                append_claude_jsonl_line(&mut file, &line)?;
                summary.appended_lines += 1;
            }
        }

        if let Some(plan) = &codex_plan {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(codex_path)?;

            for event in &plan.events {
                let line = codex_token_count_line(target_day, event)?;
                append_codex_jsonl_line(&mut file, &line)?;
                summary.appended_lines += 1;
            }
        }

        state.last_generated_day = Some(target_day.format("%Y-%m-%d").to_string());
        checkpoint_state_atomic(state_path, &state)?;

        summary.processed_days += 1;
        summary.last_generated_day = Some(target_day);
    }

    Ok(summary)
}

fn parse_day(day: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()
}

fn format_rfc3339_seconds(timestamp: i64) -> Result<String> {
    let dt = Utc
        .timestamp_opt(timestamp, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("invalid unix timestamp seconds: {}", timestamp))?;
    Ok(dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn claude_assistant_line(day: NaiveDate, event: &crate::events::InternalEvent) -> Result<String> {
    Ok(json!({
        "day": day.format("%Y-%m-%d").to_string(),
        "type": "assistant",
        "timestamp": format_rfc3339_seconds(event.timestamp)?,
        "message": {
            "model": "claude-opus-4-7-20250805",
            "usage": {
                "input_tokens": event.input_tokens,
                "output_tokens": event.output_tokens,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 0
            }
        }
    })
    .to_string())
}

fn codex_token_count_line(day: NaiveDate, event: &crate::events::InternalEvent) -> Result<String> {
    Ok(json!({
        "day": day.format("%Y-%m-%d").to_string(),
        "type": "event_msg",
        "timestamp": format_rfc3339_seconds(event.timestamp)?,
        "payload": {
            "type": "token_count",
            "model": "gpt-4o-mini",
            "info": {
                "last_token_usage": {
                    "input_tokens": event.input_tokens,
                    "output_tokens": event.output_tokens,
                    "cached_input_tokens": 0,
                    "reasoning_output_tokens": 0
                }
            }
        }
    })
    .to_string())
}

pub fn today_local() -> NaiveDate {
    Local::now().date_naive()
}

pub fn state_for_day(day: NaiveDate) -> EmulatorState {
    EmulatorState {
        last_generated_day: Some(day.format("%Y-%m-%d").to_string()),
    }
}

