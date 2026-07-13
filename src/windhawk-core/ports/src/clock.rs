//! The clock port: time as an effect, so time-dependent logic (profile
//! timestamps, last-own-write tracking) is testable without sleeping.

pub trait Clock: Send + Sync {
    /// Milliseconds since the Unix epoch.
    fn now_ms(&self) -> i64;
}
