//! The log pane's bounded tail buffer: a fixed-size ring of the most recent
//! lines, so the pane can be populated with the backlog when it is first
//! revealed - in particular the compiler-output lines pushed just before the
//! pane is revealed on a failed compile (the compiler-output surface), which a
//! live-only stream would miss. "Minimal" means a tail, not persistence: once
//! `MAX_LINES` is reached the oldest line is dropped.

use std::collections::VecDeque;
use std::sync::Mutex;

/// The retained tail length. Bounds memory for a long-running capture while keeping
/// enough scrollback to be useful; the live view auto-scrolls regardless.
const MAX_LINES: usize = 5000;

/// A `Mutex`-guarded ring of recent log lines. Shared (`Arc`) between the capture
/// thread (which pushes), the `report_op_failure` compiler-output path, and the
/// backlog command the window calls on load.
#[derive(Default)]
pub struct TailBuffer {
    lines: Mutex<VecDeque<String>>,
}

impl TailBuffer {
    /// Append one line, evicting the oldest when the tail is full.
    pub fn push(&self, line: String) {
        let mut lines = self.lock();
        if lines.len() >= MAX_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    /// A copy of the retained tail in order, for populating the pane on first reveal.
    /// (The pane's "clear" affordance clears its own view client-side; the backlog
    /// persists so a later reveal still shows recent history.)
    pub fn snapshot(&self) -> Vec<String> {
        self.lock().iter().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<String>> {
        // A poisoned lock still holds usable lines; recover rather than panic and
        // take down the capture thread.
        self.lines.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_snapshot_preserve_order() {
        let buffer = TailBuffer::default();
        buffer.push("a".to_owned());
        buffer.push("b".to_owned());
        assert_eq!(buffer.snapshot(), vec!["a".to_owned(), "b".to_owned()]);
    }

    #[test]
    fn the_tail_is_bounded_to_the_most_recent_lines() {
        let buffer = TailBuffer::default();
        for i in 0..(MAX_LINES + 10) {
            buffer.push(i.to_string());
        }
        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.len(), MAX_LINES);
        // The oldest 10 were evicted; the newest line is retained last.
        assert_eq!(snapshot.first().unwrap(), "10");
        assert_eq!(snapshot.last().unwrap(), &(MAX_LINES + 9).to_string());
    }
}
