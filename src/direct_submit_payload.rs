use anyhow::Result;
use chrono::{Datelike, NaiveDate, Utc};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde_json::json;

use crate::allocator::split_daily_tokens;
use crate::config::{AppConfig, ClientMode};

const CLAUDE_INPUT_PER_M: f64 = 5.0;
const CLAUDE_OUTPUT_PER_M: f64 = 25.0;
const CODEX_INPUT_PER_M: f64 = 0.15;
const CODEX_OUTPUT_PER_M: f64 = 0.60;

#[derive(Default)]
struct DayAgg {
    claude_input: u64,
    claude_output: u64,
    claude_messages: u64,
    codex_input: u64,
    codex_output: u64,
    codex_messages: u64,
}

pub fn build_direct_submit_payload(cfg: &AppConfig, from: NaiveDate, to: NaiveDate) -> Result<serde_json::Value> {
    let mut by_day = std::collections::BTreeMap::<String, DayAgg>::new();

    let mut day = from;
    while day <= to {
        let day_seed = cfg.runtime.seed ^ (day.num_days_from_ce() as u64) ^ 0xD1EC7;
        let mut rng = ChaCha8Rng::seed_from_u64(day_seed);
        let daily_tokens = rng.gen_range(cfg.daily_target.min_tokens..=cfg.daily_target.max_tokens);
        let (claude_tokens, codex_tokens) = split_daily_tokens(
            daily_tokens,
            cfg.clients_mix.claude_share,
            cfg.clients_mix.codex_share,
            cfg.client_mode,
        );

        let mut agg = DayAgg::default();
        if claude_tokens > 0 {
            agg.claude_input = (claude_tokens * 3) / 4;
            agg.claude_output = claude_tokens.saturating_sub(agg.claude_input);
            agg.claude_messages = (claude_tokens / 6_000).max(1);
        }
        if codex_tokens > 0 {
            agg.codex_input = (codex_tokens * 3) / 4;
            agg.codex_output = codex_tokens.saturating_sub(agg.codex_input);
            agg.codex_messages = (codex_tokens / 6_000).max(1);
        }

        by_day.insert(day.format("%Y-%m-%d").to_string(), agg);
        day = day.succ_opt().expect("date overflow");
    }

    Ok(build_payload_from_day_aggs(&by_day, cfg.client_mode))
}

fn build_payload_from_day_aggs(
    by_day: &std::collections::BTreeMap<String, DayAgg>,
    client_mode: ClientMode,
) -> serde_json::Value {
    let mut clients_set = std::collections::BTreeSet::<String>::new();
    let mut models_set = std::collections::BTreeSet::<String>::new();
    let mut contributions = Vec::<serde_json::Value>::new();

    for (day, agg) in by_day {
        let input = agg.claude_input + agg.codex_input;
        let output = agg.claude_output + agg.codex_output;
        let tokens = input + output;
        let messages = agg.claude_messages + agg.codex_messages;

        let claude_cost = (agg.claude_input as f64 / 1_000_000.0) * CLAUDE_INPUT_PER_M
            + (agg.claude_output as f64 / 1_000_000.0) * CLAUDE_OUTPUT_PER_M;
        let codex_cost = (agg.codex_input as f64 / 1_000_000.0) * CODEX_INPUT_PER_M
            + (agg.codex_output as f64 / 1_000_000.0) * CODEX_OUTPUT_PER_M;
        let day_cost = claude_cost + codex_cost;

        let mut clients = Vec::<serde_json::Value>::new();
        if agg.claude_messages > 0 || matches!(client_mode, ClientMode::ClaudeOnly) {
            clients_set.insert("claude".to_string());
            models_set.insert("claude-opus-4-7-20250805".to_string());
            clients.push(json!({
                "client": "claude",
                "modelId": "claude-opus-4-7-20250805",
                "providerId": "anthropic",
                "tokens": {
                    "input": agg.claude_input,
                    "output": agg.claude_output,
                    "cacheRead": 0,
                    "cacheWrite": 0,
                    "reasoning": 0
                },
                "cost": claude_cost,
                "messages": agg.claude_messages
            }));
        }
        if agg.codex_messages > 0 || matches!(client_mode, ClientMode::CodexOnly) {
            clients_set.insert("codex".to_string());
            models_set.insert("gpt-4o-mini".to_string());
            clients.push(json!({
                "client": "codex",
                "modelId": "gpt-4o-mini",
                "providerId": "openai",
                "tokens": {
                    "input": agg.codex_input,
                    "output": agg.codex_output,
                    "cacheRead": 0,
                    "cacheWrite": 0,
                    "reasoning": 0
                },
                "cost": codex_cost,
                "messages": agg.codex_messages
            }));
        }

        contributions.push(json!({
            "date": day,
            "totals": {
                "tokens": tokens,
                "cost": day_cost,
                "messages": messages
            },
            "intensity": if tokens > 0 { 1 } else { 0 },
            "tokenBreakdown": {
                "input": input,
                "output": output,
                "cacheRead": 0,
                "cacheWrite": 0,
                "reasoning": 0
            },
            "clients": clients
        }));
    }

    let total_tokens: u64 = contributions
        .iter()
        .map(|c| {
            c.get("totals")
                .and_then(serde_json::Value::as_object)
                .and_then(|t| t.get("tokens"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        })
        .sum();

    let total_cost: f64 = contributions
        .iter()
        .map(|c| {
            c.get("totals")
                .and_then(serde_json::Value::as_object)
                .and_then(|t| t.get("cost"))
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0)
        })
        .sum();

    let total_days = contributions.len() as u64;
    let active_days = contributions
        .iter()
        .filter(|c| {
            c.get("totals")
                .and_then(serde_json::Value::as_object)
                .and_then(|t| t.get("tokens"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
        })
        .count() as u64;

    let max_cost_in_single_day = contributions
        .iter()
        .map(|c| {
            c.get("totals")
                .and_then(serde_json::Value::as_object)
                .and_then(|t| t.get("cost"))
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0)
        })
        .fold(0.0_f64, f64::max);

    let average_per_day = if active_days > 0 {
        total_cost / active_days as f64
    } else {
        0.0
    };

    let date_range = if let (Some(start), Some(end)) = (by_day.keys().next(), by_day.keys().last()) {
        json!({ "start": start, "end": end })
    } else {
        json!({ "start": "", "end": "" })
    };

    let mut years_map = std::collections::BTreeMap::<String, (u64, f64)>::new();
    for contribution in &contributions {
        let day = contribution
            .get("date")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let year = day.split('-').next().unwrap_or("").to_string();
        let tokens = contribution
            .get("totals")
            .and_then(serde_json::Value::as_object)
            .and_then(|t| t.get("tokens"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let cost = contribution
            .get("totals")
            .and_then(serde_json::Value::as_object)
            .and_then(|t| t.get("cost"))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let entry = years_map.entry(year).or_insert((0, 0.0));
        entry.0 += tokens;
        entry.1 += cost;
    }

    let years: Vec<serde_json::Value> = years_map
        .into_iter()
        .map(|(year, (total_tokens, total_cost))| {
            json!({
                "year": year,
                "totalTokens": total_tokens,
                "totalCost": total_cost,
                "range": date_range.clone()
            })
        })
        .collect();

    json!({
        "meta": {
            "generatedAt": Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "version": "emulator-v1",
            "dateRange": date_range
        },
        "summary": {
            "totalTokens": total_tokens,
            "totalCost": total_cost,
            "totalDays": total_days,
            "activeDays": active_days,
            "averagePerDay": average_per_day,
            "maxCostInSingleDay": max_cost_in_single_day,
            "clients": clients_set.into_iter().collect::<Vec<_>>(),
            "models": models_set.into_iter().collect::<Vec<_>>()
        },
        "years": years,
        "contributions": contributions
    })
}
