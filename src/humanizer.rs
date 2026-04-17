use chrono::{Datelike, NaiveDate};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::events::{DayPlan, InternalEvent};

pub fn build_day_plan(day: NaiveDate, tokens: u64, seed: u64) -> DayPlan {
    let day_seed = seed ^ (day.num_days_from_ce() as u64);
    let mut rng = ChaCha8Rng::seed_from_u64(day_seed);

    let mut events = Vec::new();
    let mut remaining = tokens;

    let start_hour = 8 + rng.gen_range(0_u32..=3_u32);
    let start_minute = rng.gen_range(0_u32..=50_u32);
    let start_second = rng.gen_range(0_u32..=59_u32);

    let mut ts = day
        .and_hms_opt(start_hour, start_minute, start_second)
        .expect("valid daytime")
        .and_utc()
        .timestamp();

    while remaining > 0 {
        let chunk = remaining.min(rng.gen_range(500_u64..=5000_u64));
        events.push(InternalEvent {
            timestamp: ts,
            input_tokens: chunk,
            output_tokens: chunk / 3,
        });
        remaining -= chunk;

        if remaining > 0 {
            let gap = if rng.gen_bool(0.15) {
                rng.gen_range(5400_i64..=14400_i64)
            } else {
                rng.gen_range(120_i64..=3600_i64)
            };
            ts += gap;
        }
    }

    DayPlan { events }
}
