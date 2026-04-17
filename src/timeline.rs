use chrono::{Days, NaiveDate};

pub fn resolve_days(
    start: NaiveDate,
    last_completed: Option<NaiveDate>,
    today: NaiveDate,
) -> Vec<NaiveDate> {
    let mut cur = last_completed
        .map(|d| d.checked_add_days(Days::new(1)).expect("date overflow"))
        .unwrap_or(start);

    let mut out = Vec::new();
    while cur <= today {
        out.push(cur);
        cur = cur.checked_add_days(Days::new(1)).expect("date overflow");
    }

    out
}
