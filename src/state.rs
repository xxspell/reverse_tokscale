use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EmulatorState {
    pub last_generated_day: Option<String>,
}

pub fn load_state(path: &Path) -> Result<Option<EmulatorState>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)?;
    let state: EmulatorState = serde_json::from_str(&content)?;
    Ok(Some(state))
}

pub fn checkpoint_state_atomic(path: &Path, state: &EmulatorState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = path.with_extension("tmp");
    let content = serde_json::to_string_pretty(state)?;
    fs::write(&temp_path, content)?;
    fs::rename(temp_path, path)?;
    Ok(())
}
