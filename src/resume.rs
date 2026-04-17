use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::Deserialize;

use crate::state::EmulatorState;

#[derive(Debug, Deserialize)]
struct JsonlEvent {
    day: String,
}

pub fn recover_state_from_jsonl(path: &Path) -> Result<EmulatorState> {
    if !path.exists() {
        return Ok(EmulatorState::default());
    }

    let content = fs::read_to_string(path)?;
    let last_day = content
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<JsonlEvent>(line).ok())
        .map(|event| event.day);

    Ok(EmulatorState {
        last_generated_day: last_day,
    })
}

pub fn recover_state_from_jsonl_pair(
    claude_path: &Path,
    codex_path: &Path,
) -> Result<EmulatorState> {
    let claude_state = recover_state_from_jsonl(claude_path)?;
    let codex_state = recover_state_from_jsonl(codex_path)?;

    let last_generated_day = match (
        claude_state.last_generated_day,
        codex_state.last_generated_day,
    ) {
        (Some(c), Some(x)) => Some(if c >= x { c } else { x }),
        (Some(c), None) => Some(c),
        (None, Some(x)) => Some(x),
        (None, None) => None,
    };

    Ok(EmulatorState { last_generated_day })
}
