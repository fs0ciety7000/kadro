// SPDX-License-Identifier: GPL-3.0-or-later

//! Injectable clock: code that reads the time takes a `&dyn Clock`.

use jiff::Zoned;

/// Source of the current local date and time.
pub trait Clock {
    /// Current time in the local time zone.
    fn now(&self) -> Zoned;
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Zoned {
        Zoned::now()
    }
}

/// A clock frozen on a given instant, for tests.
#[derive(Debug, Clone)]
pub struct FixedClock(Zoned);

impl FixedClock {
    /// Freezes the clock on `instant`.
    #[must_use]
    pub fn new(instant: Zoned) -> Self {
        Self(instant)
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Zoned {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, FixedClock, SystemClock};

    #[test]
    fn fixed_clock_does_not_move() {
        let at: jiff::Zoned = "2026-11-11T09:08:07+01:00[Europe/Brussels]".parse().unwrap();
        let clock = FixedClock::new(at.clone());
        assert_eq!(clock.now(), at);
        assert_eq!(clock.now(), at);
    }

    #[test]
    fn system_clock_is_recent() {
        let year = SystemClock.now().year();
        assert!(year >= 2026);
    }
}
