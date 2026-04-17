use tokscale_activity_emulator::events::InternalEvent;
use tokscale_activity_emulator::validator::validate_events;

#[test]
fn rejects_non_monotonic_timestamps() {
    let events = vec![
        InternalEvent {
            timestamp: 2,
            input_tokens: 10,
            output_tokens: 1,
        },
        InternalEvent {
            timestamp: 1,
            input_tokens: 10,
            output_tokens: 1,
        },
    ];

    assert!(validate_events(&events).is_err());
}
