use anyhow::{bail, Result};

use crate::events::InternalEvent;

pub fn validate_events(events: &[InternalEvent]) -> Result<()> {
    for window in events.windows(2) {
        if window[0].timestamp >= window[1].timestamp {
            bail!("timestamps must be strictly increasing");
        }
    }

    Ok(())
}
