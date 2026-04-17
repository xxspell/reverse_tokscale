use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use chrono::NaiveDate;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub daily_target: DailyTarget,
    pub clients_mix: ClientsMix,
    pub client_mode: ClientMode,
    pub runtime: RuntimeConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DailyTarget {
    pub min_tokens: u64,
    pub max_tokens: u64,
    pub hard_cap_tokens: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientsMix {
    pub claude_share: f64,
    pub codex_share: f64,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ClientMode {
    Both,
    ClaudeOnly,
    CodexOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub start_day: Option<NaiveDate>,
    pub seed: u64,
    pub submit_payload_output_path: Option<String>,
    pub submit_state_path: Option<String>,
}

impl AppConfig {
    pub fn from_yaml_str(s: &str) -> Result<Self> {
        let cfg: Self = serde_yaml::from_str(s)?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn from_yaml_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::from_yaml_str(&content)
    }

    pub fn validate(&self) -> Result<()> {
        if self.daily_target.min_tokens > self.daily_target.max_tokens {
            bail!("daily_target.min_tokens must be <= max_tokens");
        }

        if self.daily_target.max_tokens > self.daily_target.hard_cap_tokens {
            bail!("daily_target.max_tokens must be <= hard_cap_tokens");
        }

        if self.daily_target.min_tokens == 0 {
            bail!("daily_target.min_tokens must be > 0");
        }

        if self.daily_target.hard_cap_tokens == 0 {
            bail!("daily_target.hard_cap_tokens must be > 0");
        }

        Self::validate_share(self.clients_mix.claude_share, "clients_mix.claude_share")?;
        Self::validate_share(self.clients_mix.codex_share, "clients_mix.codex_share")?;

        let sum = self.clients_mix.claude_share + self.clients_mix.codex_share;
        if (sum - 1.0).abs() > 1e-9 {
            bail!("clients_mix shares must sum to 1.0");
        }

        match self.client_mode {
            ClientMode::Both => {
                if self.clients_mix.claude_share <= 0.0 || self.clients_mix.codex_share <= 0.0 {
                    bail!("both mode requires positive shares for both clients");
                }
            }
            ClientMode::ClaudeOnly => {
                if self.clients_mix.claude_share != 1.0 || self.clients_mix.codex_share != 0.0 {
                    bail!("claude_only mode requires claude_share=1.0 and codex_share=0.0");
                }
            }
            ClientMode::CodexOnly => {
                if self.clients_mix.claude_share != 0.0 || self.clients_mix.codex_share != 1.0 {
                    bail!("codex_only mode requires claude_share=0.0 and codex_share=1.0");
                }
            }
        }

        if let Some(path) = &self.runtime.submit_payload_output_path {
            Self::validate_runtime_path(path, "runtime.submit_payload_output_path")?;
        }
        if let Some(path) = &self.runtime.submit_state_path {
            Self::validate_runtime_path(path, "runtime.submit_state_path")?;
        }

        Ok(())
    }

    pub fn resolved_submit_payload_output_path(&self) -> Result<Option<PathBuf>> {
        self.runtime
            .submit_payload_output_path
            .as_deref()
            .map(|p| Self::expand_home_path(p, "runtime.submit_payload_output_path"))
            .transpose()
    }

    pub fn resolved_submit_state_path(&self) -> Result<Option<PathBuf>> {
        self.runtime
            .submit_state_path
            .as_deref()
            .map(|p| Self::expand_home_path(p, "runtime.submit_state_path"))
            .transpose()
    }

    fn validate_runtime_path(value: &str, field: &str) -> Result<()> {
        if value.trim().is_empty() {
            bail!("{field} must not be empty");
        }
        Ok(())
    }

    fn expand_home_path(value: &str, field: &str) -> Result<PathBuf> {
        if let Some(stripped) = value.strip_prefix("~/") {
            let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
            return Ok(PathBuf::from(home).join(stripped));
        }

        if value == "~" {
            let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
            return Ok(PathBuf::from(home));
        }

        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Ok(path);
        }

        bail!("{field} must be an absolute path or start with ~/")
    }

    fn validate_share(value: f64, field: &str) -> Result<()> {
        if !value.is_finite() {
            bail!("{field} must be finite");
        }
        if !(0.0..=1.0).contains(&value) {
            bail!("{field} must be within [0.0, 1.0]");
        }
        Ok(())
    }
}
