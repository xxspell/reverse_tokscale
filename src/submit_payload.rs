use std::fs;
use std::path::Path;

use anyhow::Result;
use chrono::Utc;
use serde_json::json;

#[derive(Default)]
struct DayAgg {
    claude_input: u64,
    claude_output: u64,
    claude_messages: u64,
    codex_input: u64,
    codex_output: u64,
    codex_messages: u64,
}

fn read_jsonl_lines(path: &Path) -> Result<Vec<serde_json::Value>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(path)?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn build_submit_payload(claude_path: &Path, codex_path: &Path) -> Result<serde_json::Value> {
    let mut by_day = std::collections::BTreeMap::<String, DayAgg>::new();

    for line in read_jsonl_lines(claude_path)? {
        let Some(day) = line
            .get("day")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };

        let input = line
            .get("message")
            .and_then(serde_json::Value::as_object)
            .and_then(|m| m.get("usage"))
            .and_then(serde_json::Value::as_object)
            .and_then(|u| u.get("input_tokens"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let output = line
            .get("message")
            .and_then(serde_json::Value::as_object)
            .and_then(|m| m.get("usage"))
            .and_then(serde_json::Value::as_object)
            .and_then(|u| u.get("output_tokens"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);

        let agg = by_day.entry(day).or_default();
        agg.claude_input += input;
        agg.claude_output += output;
        agg.claude_messages += 1;
    }

    for line in read_jsonl_lines(codex_path)? {
        let Some(day) = line
            .get("day")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };

        let input = line
            .get("payload")
            .and_then(serde_json::Value::as_object)
            .and_then(|p| p.get("info"))
            .and_then(serde_json::Value::as_object)
            .and_then(|i| i.get("last_token_usage"))
            .and_then(serde_json::Value::as_object)
            .and_then(|u| u.get("input_tokens"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let output = line
            .get("payload")
            .and_then(serde_json::Value::as_object)
            .and_then(|p| p.get("info"))
            .and_then(serde_json::Value::as_object)
            .and_then(|i| i.get("last_token_usage"))
            .and_then(serde_json::Value::as_object)
            .and_then(|u| u.get("output_tokens"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);

        let agg = by_day.entry(day).or_default();
        agg.codex_input += input;
        agg.codex_output += output;
        agg.codex_messages += 1;
    }

    let mut clients_set = std::collections::BTreeSet::<String>::new();
    let mut models_set = std::collections::BTreeSet::<String>::new();
    let mut contributions = Vec::<serde_json::Value>::new();

    const CLAUDE_INPUT_PER_M: f64 = 5.0;
    const CLAUDE_OUTPUT_PER_M: f64 = 25.0;
    const CODEX_INPUT_PER_M: f64 = 0.15;
    const CODEX_OUTPUT_PER_M: f64 = 0.60;

    for (day, agg) in &by_day {
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
        if agg.claude_messages > 0 {
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
        if agg.codex_messages > 0 {
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

    Ok(json!({
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
    }))
}

pub fn write_submit_payload(payload_path: &Path, claude_path: &Path, codex_path: &Path) -> Result<()> {
    if let Some(parent) = payload_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = build_submit_payload(claude_path, codex_path)?;
    fs::write(payload_path, serde_json::to_string_pretty(&payload)?)?;
    Ok(())
}
