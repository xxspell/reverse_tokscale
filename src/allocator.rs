use crate::config::ClientMode;

pub fn split_daily_tokens(
    total: u64,
    claude_share: f64,
    codex_share: f64,
    mode: ClientMode,
) -> (u64, u64) {
    match mode {
        ClientMode::ClaudeOnly => (total, 0),
        ClientMode::CodexOnly => (0, total),
        ClientMode::Both => {
            let total_shares = claude_share + codex_share;
            if !total_shares.is_finite() || total_shares <= 0.0 {
                return (0, total);
            }

            let normalized_claude = claude_share / total_shares;
            let claude = ((total as f64) * normalized_claude).round() as u64;
            (claude, total.saturating_sub(claude))
        }
    }
}
