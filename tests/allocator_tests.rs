use tokscale_activity_emulator::allocator::split_daily_tokens;
use tokscale_activity_emulator::config::ClientMode;

#[test]
fn splits_tokens_for_both_clients() {
    let (c, x) = split_daily_tokens(1_000, 0.6, 0.4, ClientMode::Both);
    assert_eq!(c + x, 1_000);
    assert!((599..=601).contains(&c));
}

#[test]
fn claude_only_routes_all_tokens_to_claude() {
    let (c, x) = split_daily_tokens(1_000, 0.6, 0.4, ClientMode::ClaudeOnly);
    assert_eq!(c, 1_000);
    assert_eq!(x, 0);
}

#[test]
fn codex_only_routes_all_tokens_to_codex() {
    let (c, x) = split_daily_tokens(1_000, 0.6, 0.4, ClientMode::CodexOnly);
    assert_eq!(c, 0);
    assert_eq!(x, 1_000);
}
