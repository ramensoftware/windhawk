//! Cross-process mutual exclusion: a named Win32 mutex, introduced for exactly
//! one artifact - the user-profile read-modify-write. This is new behavior,
//! deliberately scoped to core sessions and best effort: it serializes core
//! sessions against each other, and during the migration window the TypeScript
//! backend and the C++ app do not take it (so a failure to acquire degrades to
//! today's last-write-wins rather than failing the command).

/// Acquire a named cross-process lock.
pub trait NamedLock: Send + Sync {
    /// Acquire `name`, waiting up to `timeout_ms`. Always returns a guard; on
    /// timeout or OS failure the guard is simply unheld (callers proceed - the
    /// lock only coordinates core sessions). Dropping the guard releases it.
    fn acquire(&self, name: &str, timeout_ms: u32) -> Box<dyn NamedLockGuard>;
}

/// An RAII guard for a held (or, on failure, best-effort-unheld) named lock;
/// the lock is released when the guard is dropped. Not `Send`: a guard is held
/// only within a single synchronous command's read-modify-write on the caller
/// thread (a Win32 mutex must be released by the thread that acquired it).
pub trait NamedLockGuard {}
