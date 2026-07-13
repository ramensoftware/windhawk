//! Production clock adapter.

use std::time::{SystemTime, UNIX_EPOCH};

use windhawk_core_ports::Clock;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        // A pre-1970 system clock saturates to 0 rather than panicking.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_after_2020() {
        assert!(SystemClock.now_ms() > 1_577_836_800_000);
    }
}
