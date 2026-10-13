use chrono::{DateTime, Utc};

use crate::application::ports::Clock;

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_is_close_to_utc_now() {
        let before = Utc::now();
        let now = SystemClock.now();
        assert!(now >= before);
    }
}
