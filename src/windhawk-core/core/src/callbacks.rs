//! The callback dispatcher: one thread per session owning an MPSC queue; every
//! log line and operation event is delivered from that thread and no other.
//! This gives the ABI its callback guarantees cheaply: callbacks never fire on
//! a thread inside an invoke, per-session callbacks are totally ordered, and
//! callbacks cannot deadlock the session (the dispatcher holds no lock while
//! calling out).

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

/// Log levels of the `WhCoreLogCallback` contract: 0=error, 1=warn, 2=info.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
}

pub type LogFn = Box<dyn Fn(LogLevel, &str) + Send>;
pub type EventFn = Box<dyn Fn(u64, &str) + Send>;

/// The host callbacks of `WhCoreSessionCreate`, wrapped into safe closures
/// by the FFI crate (the unsafe pointer call lives there; services never
/// see these - they go through the dispatcher).
pub struct HostCallbacks {
    pub log: LogFn,
    pub event: EventFn,
}

enum Item {
    Log(LogLevel, String),
    Event(u64, String),
    Shutdown,
}

pub struct CallbackDispatcher {
    tx: Sender<Item>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl CallbackDispatcher {
    pub fn new(callbacks: HostCallbacks) -> Self {
        let (tx, rx): (Sender<Item>, Receiver<Item>) = channel();
        let thread = std::thread::Builder::new()
            .name("windhawk-core callback dispatcher".into())
            .spawn(move || {
                while let Ok(item) = rx.recv() {
                    match item {
                        Item::Log(level, message) => (callbacks.log)(level, &message),
                        Item::Event(op_id, json) => (callbacks.event)(op_id, &json),
                        Item::Shutdown => break,
                    }
                }
            })
            .ok();
        Self {
            tx,
            thread: Mutex::new(thread),
        }
    }

    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        // A send error means shutdown already drained the queue; per the
        // destroy contract nothing may be delivered after that anyway.
        let _ = self.tx.send(Item::Log(level, message.into()));
    }

    pub fn event(&self, op_id: u64, event_json: String) {
        let _ = self.tx.send(Item::Event(op_id, event_json));
    }

    /// Deliver everything queued so far, then stop; no callback fires after
    /// this returns. Callers must have joined every emitting thread first.
    pub fn shutdown(&self) {
        let _ = self.tx.send(Item::Shutdown);
        let thread = {
            let mut guard = self.thread.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn delivers_in_order_and_nothing_after_shutdown() {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let l = log.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let dispatcher = CallbackDispatcher::new(HostCallbacks {
            log: Box::new(move |level, msg| {
                l.lock().unwrap().push(format!("{:?}:{msg}", level));
            }),
            event: Box::new(move |_, _| {
                c.fetch_add(1, Ordering::SeqCst);
            }),
        });
        dispatcher.log(LogLevel::Info, "a");
        dispatcher.event(1, "{}".into());
        dispatcher.log(LogLevel::Error, "b");
        dispatcher.shutdown();
        assert_eq!(*log.lock().unwrap(), vec!["Info:a", "Error:b"]);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        // Late emissions are dropped, not delivered.
        dispatcher.log(LogLevel::Info, "late");
        assert_eq!(log.lock().unwrap().len(), 2);
    }
}
