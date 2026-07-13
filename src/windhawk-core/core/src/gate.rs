//! The shutdown gate: every external entry into the session holds a gate guard;
//! destroy closes the gate and waits until in-flight calls have drained. The
//! ABI already forbids calling into a session being destroyed; the gate makes
//! the in-flight case deterministic rather than undefined.

use std::sync::{Condvar, Mutex};

#[derive(Default)]
struct GateState {
    closed: bool,
    in_flight: u32,
}

#[derive(Default)]
pub struct ShutdownGate {
    state: Mutex<GateState>,
    cv: Condvar,
}

pub struct GateGuard<'a> {
    gate: &'a ShutdownGate,
}

impl ShutdownGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enter the gate; `None` once shutdown has begun.
    pub fn enter(&self) -> Option<GateGuard<'_>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed {
            return None;
        }
        state.in_flight += 1;
        Some(GateGuard { gate: self })
    }

    /// Close the gate and wait for in-flight calls to drain.
    pub fn close_and_wait(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        while state.in_flight > 0 {
            state = self.cv.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Drop for GateGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.gate.state.lock().unwrap_or_else(|e| e.into_inner());
        state.in_flight -= 1;
        if state.in_flight == 0 && state.closed {
            self.gate.cv.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn close_waits_for_in_flight_guards() {
        let gate = Arc::new(ShutdownGate::new());
        let guard = gate.enter();
        assert!(guard.is_some());

        let g = gate.clone();
        let closer = std::thread::spawn(move || g.close_and_wait());
        std::thread::sleep(Duration::from_millis(30));
        assert!(!closer.is_finished());

        drop(guard);
        closer.join().unwrap();
        assert!(gate.enter().is_none());
    }
}
