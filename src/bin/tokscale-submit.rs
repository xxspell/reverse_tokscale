use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration as StdDuration;

use anyhow::{bail, Context, Result};
use chrono::{Duration, Local, NaiveDate};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;
use tokscale_submit_tool::config::AppConfig;
use tokscale_submit_tool::direct_submit_payload::build_direct_submit_payload;
use tokscale_submit_tool::submit_state::{
    checkpoint_submit_state_atomic, load_submit_state, SubmitState,
};

#[derive(Debug, Deserialize)]
struct Credentials {
    token: String,
}

fn parse_day(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(Into::into)
}

fn must_next(args: &[String], i: usize, flag: &str) -> Result<String> {
    args.get(i + 1)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))
}

fn resolve_project_dir(config_path: &Path) -> Result<PathBuf> {
    let abs_config = if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(config_path)
    };

    let config_dir = abs_config
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("config path has no parent: {}", abs_config.display()))?;

    if config_dir.file_name().and_then(|s| s.to_str()) == Some("configs") {
        return Ok(config_dir.parent().unwrap_or(&config_dir).to_path_buf());
    }

    Ok(config_dir)
}

fn resolve_submit_state_path(cfg: &AppConfig, project_dir: &Path) -> Result<PathBuf> {
    if let Some(path) = cfg.resolved_submit_state_path()? {
        return Ok(path);
    }

    Ok(project_dir.join(".tokscale/submit-state.json"))
}

fn resolve_submit_payload_path(cfg: &AppConfig, project_dir: &Path) -> Result<PathBuf> {
    if let Some(path) = cfg.resolved_submit_payload_output_path()? {
        return Ok(path);
    }

    Ok(project_dir.join(".tokscale/submit-payload.json"))
}

fn load_credentials(project_dir: &Path) -> Result<(String, String)> {
    let api_url = std::env::var("TOKSCALE_API_URL").unwrap_or_else(|_| "https://tokscale.ai".to_string());

    if let Ok(token) = std::env::var("TOKSCALE_TOKEN") {
        if !token.trim().is_empty() {
            return Ok((api_url, token));
        }
    }

    let credentials_path = project_dir.join(".tokscale/credentials.json");
    let content = fs::read_to_string(&credentials_path)
        .with_context(|| format!("failed to read credentials at {}", credentials_path.display()))?;
    let creds: Credentials = serde_json::from_str(&content)?;
    Ok((api_url, creds.token))
}

fn payload_contributions_count(payload: &Value) -> usize {
    payload
        .get("contributions")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0)
}

fn post_submit(api_url: &str, token: &str, payload: &Value) -> Result<()> {
    let url = format!("{}/api/submit", api_url.trim_end_matches('/'));
    let attempts = std::env::var("TOKSCALE_SUBMIT_RETRIES")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(5);
    let timeout_secs = std::env::var("TOKSCALE_SUBMIT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(90);
    let client = Client::builder()
        .timeout(StdDuration::from_secs(timeout_secs))
        .build()?;

    for attempt in 1..=attempts {
        let send_result = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(payload)
            .send();

        match send_result {
            Ok(res) if res.status().is_success() => return Ok(()),
            Ok(res) => {
                let status = res.status();
                let body = res.text().unwrap_or_else(|_| "<failed to read body>".to_string());
                if status.is_client_error() {
                    bail!("submit rejected: {status} {body}");
                }
                if attempt == attempts {
                    bail!("submit failed after {attempts} attempts: {status} {body}");
                }
                let delay = 1_u64 << (attempt - 1);
                println!(
                    "[tokscale-submit] submit attempt {attempt}/{attempts} failed: {status}; retry in {delay}s"
                );
                thread::sleep(StdDuration::from_secs(delay));
            }
            Err(err) => {
                if attempt == attempts {
                    bail!("submit request failed after {attempts} attempts: {err}");
                }
                let delay = 1_u64 << (attempt - 1);
                println!(
                    "[tokscale-submit] submit attempt {attempt}/{attempts} error: {err}; retry in {delay}s"
                );
                thread::sleep(StdDuration::from_secs(delay));
            }
        }
    }

    Ok(())
}

fn cmd_run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("usage: tokscale-submit run <config.yaml> [--from YYYY-MM-DD] [--to YYYY-MM-DD] [--shard-days N]");
    }

    let config_path = PathBuf::from(&args[0]);
    let mut from_arg: Option<NaiveDate> = None;
    let mut to_arg: Option<NaiveDate> = None;
    let mut shard_days: i64 = 0;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--from" => {
                from_arg = Some(parse_day(&must_next(args, i, "--from")?)?);
                i += 2;
            }
            "--to" => {
                to_arg = Some(parse_day(&must_next(args, i, "--to")?)?);
                i += 2;
            }
            "--shard-days" => {
                shard_days = must_next(args, i, "--shard-days")?.parse::<i64>()?;
                i += 2;
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    let project_dir = resolve_project_dir(&config_path)?;
    let cfg = AppConfig::from_yaml_file(&config_path)?;
    let payload_path = resolve_submit_payload_path(&cfg, &project_dir)?;
    let submit_state_path = resolve_submit_state_path(&cfg, &project_dir)?;

    if let Some(parent) = submit_state_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = payload_path.parent() {
        fs::create_dir_all(parent)?;
    }

    println!("[tokscale-submit] project dir: {}", project_dir.display());
    println!("[tokscale-submit] config: {}", config_path.display());
    println!("[tokscale-submit] submit state: {}", submit_state_path.display());

    let submit_state = load_submit_state(&submit_state_path)?.unwrap_or_default();
    println!(
        "[tokscale-submit] resume checkpoint: {}",
        submit_state
            .last_submitted_day
            .as_deref()
            .unwrap_or("<none>")
    );

    let target_to = to_arg.unwrap_or_else(|| Local::now().date_naive());
    let configured_start = cfg.runtime.start_day.unwrap_or(target_to);
    let default_from = submit_state
        .last_submitted_day
        .as_deref()
        .map(parse_day)
        .transpose()?
        .map(|d| d + Duration::days(1));

    let effective_from = from_arg.or(default_from).unwrap_or(configured_start);
    if target_to < effective_from {
        println!("[tokscale-submit] nothing to submit (to < from)");
        return Ok(());
    }

    println!(
        "[tokscale-submit] direct aggregate range: {}..{}",
        effective_from.format("%Y-%m-%d"),
        target_to.format("%Y-%m-%d")
    );

    let payload = build_direct_submit_payload(&cfg, effective_from, target_to)?;
    let contributions = payload_contributions_count(&payload);
    println!("[tokscale-submit] built payload: {} contributions", contributions);

    if contributions == 0 {
        println!("[tokscale-submit] nothing to submit (0 contributions)");
        return Ok(());
    }

    fs::write(&payload_path, serde_json::to_string_pretty(&payload)?)?;
    println!("[tokscale-submit] wrote payload cache: {}", payload_path.display());

    if shard_days > 0 {
        println!(
            "[tokscale-submit] sharded run: {}..{} (shard-days={})",
            effective_from.format("%Y-%m-%d"),
            target_to.format("%Y-%m-%d"),
            shard_days
        );

        let (api_url, token) = load_credentials(&project_dir)?;
        let mut cursor = effective_from;
        while cursor <= target_to {
            let shard_to = std::cmp::min(cursor + Duration::days(shard_days - 1), target_to);
            let shard_payload = build_direct_submit_payload(&cfg, cursor, shard_to)?;
            let shard_count = payload_contributions_count(&shard_payload);
            if shard_count > 0 {
                println!(
                    "[tokscale-submit] submit shard {}..{} ({} contributions)",
                    cursor.format("%Y-%m-%d"),
                    shard_to.format("%Y-%m-%d"),
                    shard_count
                );
                post_submit(&api_url, &token, &shard_payload)?;
                checkpoint_submit_state_atomic(
                    &submit_state_path,
                    &SubmitState {
                        last_submitted_day: Some(shard_to.format("%Y-%m-%d").to_string()),
                    },
                )?;
                println!(
                    "[tokscale-submit] checkpoint updated: last_submitted_day={}",
                    shard_to.format("%Y-%m-%d")
                );
            } else {
                println!(
                    "[tokscale-submit] skip shard {}..{} (0 contributions)",
                    cursor.format("%Y-%m-%d"),
                    shard_to.format("%Y-%m-%d")
                );
            }
            cursor = shard_to + Duration::days(1);
        }
        println!("[tokscale-submit] done");
        return Ok(());
    }

    println!("[tokscale-submit] submit contributions: {}", contributions);

    let (api_url, token) = load_credentials(&project_dir)?;
    println!(
        "[tokscale-submit] submit full payload to {}/api/submit",
        api_url.trim_end_matches('/')
    );
    post_submit(&api_url, &token, &payload)?;

    checkpoint_submit_state_atomic(
        &submit_state_path,
        &SubmitState {
            last_submitted_day: Some(target_to.format("%Y-%m-%d").to_string()),
        },
    )?;
    println!(
        "[tokscale-submit] checkpoint updated: last_submitted_day={}",
        target_to.format("%Y-%m-%d")
    );

    println!("[tokscale-submit] done");
    Ok(())
}

fn cmd_reset(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        bail!("usage: tokscale-submit reset --soft|--hard <config.yaml>");
    }

    let mode = &args[0];
    let config_path = Path::new(&args[1]);
    let project_dir = resolve_project_dir(config_path)?;
    let cfg = AppConfig::from_yaml_file(config_path)?;
    let submit_state_path = resolve_submit_state_path(&cfg, &project_dir)?;

    if submit_state_path.exists() {
        fs::remove_file(&submit_state_path)?;
        println!("[tokscale-submit] removed submit state: {}", submit_state_path.display());
    } else {
        println!(
            "[tokscale-submit] submit state already absent: {}",
            submit_state_path.display()
        );
    }

    if mode == "--hard" {
        let payload_path = resolve_submit_payload_path(&cfg, &project_dir)?;
        if payload_path.exists() {
            fs::remove_file(&payload_path)?;
            println!("[tokscale-submit] removed payload cache: {}", payload_path.display());
        }
    } else if mode != "--soft" {
        bail!("unknown reset mode: {mode}");
    }

    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        bail!("usage: tokscale-submit <run|reset> ...");
    }

    match args[0].as_str() {
        "run" => cmd_run(&args[1..]),
        "reset" => cmd_reset(&args[1..]),
        other => bail!("unknown command: {other}"),
    }
}
