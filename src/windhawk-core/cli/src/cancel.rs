//! The Windows console control handler: Ctrl+C -> `WhCoreCancel` for the
//! tracked operation. The handler does the MINIMUM - it signals the cancel and
//! returns; the main thread then drains the operation to its terminal
//! `failed(CANCELED)` event, which the error path maps to exit 9 (the
//! single-mpsc async design of `client.rs`). When no operation is tracked (a
//! signal during argv parsing or a synchronous command), it exits the process
//! with the CANCELLED code directly, mirroring the TS `runCommand.handleSigint`
//! no-context path.
//!
//! The cancel capability is the host's [`CancelHandle`] (a thin `WhCoreCancel`
//! handle bound to one op-id); the handler holds it off-session and calls it
//! directly. `WhCoreCancel` is thread-safe, so the handler - which runs on an
//! OS-injected thread - may call it directly; it must not block or otherwise
//! touch the session.

use std::sync::{Arc, Mutex, OnceLock};

use windhawk_core_host::CancelHandle;

/// The CANCELLED exit code.
const CANCELLED_EXIT_CODE: i32 = 9;

type Slot = Option<Arc<dyn CancelHandle>>;

fn slot() -> &'static Mutex<Slot> {
    static SLOT: OnceLock<Mutex<Slot>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Install the console control handler once per process (from `main` via
/// `run`). A second install is ignored (`ctrlc` rejects a duplicate handler),
/// which never happens in practice.
pub fn install_handler() {
    let _ = ctrlc::set_handler(on_signal);
}

/// The handler body, on an OS-injected thread. Cancels the tracked operation if
/// there is one (signal only); otherwise exits with the CANCELLED code.
fn on_signal() {
    let cancelled = match slot().lock() {
        Ok(guard) => match guard.as_ref() {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        },
        // A poisoned lock means a panic already happened; fail toward exiting.
        Err(_) => false,
    };
    if !cancelled {
        // No tracked operation to drain (signal during parse or a sync command):
        // exit now, the TS no-context path. An in-flight operation, by contrast,
        // is left for the main thread to drain to failed(CANCELED) -> exit 9.
        std::process::exit(CANCELLED_EXIT_CODE);
    }
}

/// Track an operation's [`CancelHandle`] so the handler can cancel it. The async
/// invoke calls this before it begins draining the operation's events.
pub fn track(token: Arc<dyn CancelHandle>) {
    if let Ok(mut guard) = slot().lock() {
        *guard = Some(token);
    }
}

/// Stop tracking (the operation reached its terminal event). Drops the held
/// token; its `Arc` of the session is released, so the session is destroyed
/// deterministically when the owning `Core` drops.
pub fn untrack() {
    if let Ok(mut guard) = slot().lock() {
        *guard = None;
    }
}
