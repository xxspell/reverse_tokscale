use chrono::NaiveDate;
use tokscale_activity_emulator::timeline::resolve_days;

#[test]
fn includes_missing_days_for_catchup() {
    let start = NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 4, 4).unwrap();

    let days = resolve_days(
        start,
        Some(NaiveDate::from_ymd_opt(2026, 4, 2).unwrap()),
        today,
    );

    assert_eq!(days.len(), 2);
    assert_eq!(days[0], NaiveDate::from_ymd_opt(2026, 4, 3).unwrap());
    assert_eq!(days[1], NaiveDate::from_ymd_opt(2026, 4, 4).unwrap());
}

#[test]
fn uses_start_day_when_no_last_completed() {
    let start = NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 4, 3).unwrap();

    let days = resolve_days(start, None, today);

    assert_eq!(
        days,
        vec![
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 2).unwrap(),
            NaiveDate::from_ymd_opt(2026, 4, 3).unwrap(),
        ]
    );
}

#[test]
fn returns_empty_when_last_completed_is_today_or_later() {
    let start = NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 4, 4).unwrap();

    let same_day = resolve_days(start, Some(today), today);
    assert!(same_day.is_empty());

    let future_last = resolve_days(
        start,
        Some(NaiveDate::from_ymd_opt(2026, 4, 5).unwrap()),
        today,
    );
    assert!(future_last.is_empty());
}
