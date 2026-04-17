use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SubmitState {
    pub last_submitted_day: Option<String>,
}

pub fn load_submit_state(path: &Path) -> Result<Option<SubmitState>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)?;
    let state: SubmitState = serde_json::from_str(&content)?;
    Ok(Some(state))
}

pub fn checkpoint_submit_state_atomic(path: &Path, state: &SubmitState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = path.with_extension("tmp");
    let content = serde_json::to_string_pretty(state)?;
    fs::write(&temp_path, content)?;
    fs::rename(temp_path, path)?;
    Ok(())
}
