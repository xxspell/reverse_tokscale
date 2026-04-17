use chrono::{NaiveDate, Timelike};
use tokscale_activity_emulator::humanizer::build_day_plan;

#[test]
fn fixed_seed_produces_stable_plan() {
    let d = NaiveDate::from_ymd_opt(2026, 4, 10).unwrap();
    let a = build_day_plan(d, 100_000, 42);
    let b = build_day_plan(d, 100_000, 42);
    assert_eq!(a, b);
}

#[test]
fn timestamps_are_monotonic() {
    let d = NaiveDate::from_ymd_opt(2026, 4, 10).unwrap();
    let plan = build_day_plan(d, 100_000, 42);

    for w in plan.events.windows(2) {
        assert!(w[0].timestamp <= w[1].timestamp);
    }
}

#[test]
fn start_hour_varies_across_days() {
    let mut start_hours = std::collections::BTreeSet::new();

    for day in 1..=10 {
        let d = NaiveDate::from_ymd_opt(2026, 4, day).unwrap();
        let plan = build_day_plan(d, 100_000, 42);
        let ts = plan.events.first().expect("events").timestamp;
        let hour = chrono::DateTime::from_timestamp(ts, 0)
            .expect("valid timestamp")
            .hour();
        start_hours.insert(hour);
    }

    assert!(
        start_hours.len() > 1,
        "human-like generator should vary session start hour across days"
    );
}
