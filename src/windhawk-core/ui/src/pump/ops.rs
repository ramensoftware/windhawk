//! The op-id registry: the ONE owner of async-op correlation. Against each core
//! op-id it holds the originating `(command, messageId)`, the per-command
//! [`AsyncKind`], the captured `context`, and the cancel handle - so cancel
//! bookkeeping is not a second map. Both the bridge (which starts ops) and the
//! pump (which routes their events) reach it through one `Arc<OpRegistry>`
//! handle.
//!
//! Register/event ordering: the core never fires an op's event on the thread
//! inside `invoke_async`, but it MAY fire it on another thread before the
//! bridge - on its own worker - records the op. So an event for an unregistered
//! op is BUFFERED here; [`OpRegistry::register`] returns the buffered events
//! for the registrant to dispatch, making the early-terminal race impossible to
//! lose rather than merely improbable.
//!
//! Session GENERATIONS: an op-id is meaningful only to the session that issued
//! it, and a session allocates its ids from 1, so ids from two sessions collide
//! by construction. Every op is therefore stamped with the generation of the
//! session that started it and every event carries the generation of the session
//! that produced it, so an event only ever reaches an op of its own session
//! ([`OpRegistry::kind`], [`OpRegistry::take`]) and an event with no op to reach
//! is buffered only while its session is the installed one
//! ([`OpRegistry::buffer`]).
//!
//! A START is stamped by generation rather than by when it lands: the installed
//! generation is read BEFORE the op is started ([`OpRegistry::generation`]) and
//! handed back to [`OpRegistry::register`], which refuses an op whose session is
//! no longer the installed one. Without that, a swap landing between the start
//! and the registration would record the op under the INCOMING generation - too
//! late for [`OpRegistry::drain_and_install`] to have ended it, and stamped so
//! that its own session's events can never reach it, which is precisely the
//! shape that leaves a `messageWithReply` hanging forever. Such an op is handed
//! back instead ([`Registered::Orphaned`]) for the caller to end exactly as the
//! drain would have.
//!
//! A generation belongs to a SESSION, for that session's whole life - it is not
//! an epoch counter. So [`OpRegistry::drain_and_install`] is told which
//! generation is taking over rather than picking the next one itself, and
//! installing a generation that was installed before is ordinary: a session the
//! UI returns to arrives carrying the generation it always had. That is safe
//! because a session's ids come from a monotonic counter and are never reused
//! within it, so a late event from an op a drain already ended can only fail to
//! find its op - it can never find a DIFFERENT one. Such an event does land in
//! `pending`, under an id its session will not issue again, where it sits until
//! the next drain clears it; the residue is bounded by the ops in flight at the
//! swap.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use windhawk_core_host::CancelHandle;

use crate::ipc::outcome::AsyncKind;

/// The generation of the session the UI starts on, which a fresh [`OpRegistry`]
/// has installed. The event source stamps its events with it
/// (`lifecycle::session::start_core`), so the two agree from the first event, and
/// it stays that session's generation for the process lifetime - including across
/// a swap away and back. Every later session takes a generation of its own, one
/// per session, allocated by whoever creates it.
pub const FIRST_GENERATION: u64 = 0;

/// One registered async op: its originating correlation, per-command knowledge,
/// captured context, and cancel handle.
pub struct OpEntry {
    pub command: String,
    /// The originating `messageWithReply` id the terminal reply echoes. `0` for an
    /// internal background op (the startup refresh), whose `Terminal::Internal`
    /// emits no reply, so the id is unused.
    pub message_id: i64,
    pub kind: AsyncKind,
    pub context: Value,
    /// The op's cancel handle. Always `Some` in production (the bridge binds it to
    /// the op-id); `None` only in the headless dispatcher tests, which store but
    /// never invoke it (a handle needs a real session to construct).
    pub cancel: Option<Arc<dyn CancelHandle>>,
}

/// A registered op and the generation it was registered under.
struct Slot {
    generation: u64,
    entry: OpEntry,
}

/// What [`OpRegistry::register`] did with a started op.
pub enum Registered {
    /// Recorded. Carries the events that arrived before the call, each with the
    /// generation that produced it; the caller dispatches them, by which point the
    /// op is present.
    Replay(Vec<(u64, String)>),
    /// NOT recorded: a swap ran between the start and this call, so the op belongs
    /// to a session that is no longer installed. The drain that ends the outgoing
    /// session's ops has already been and gone, and nothing else ever will - the
    /// op's own events carry a generation this registry now refuses. The entry is
    /// handed back for the caller to end the way the drain would have.
    Orphaned(OpEntry),
}

struct Inner {
    ops: HashMap<u64, Slot>,
    /// Events that arrived before their op was registered (the register/event
    /// race), each with the generation of the session that produced it; drained by
    /// [`OpRegistry::register`].
    pending: HashMap<u64, Vec<(u64, String)>>,
    /// The generation of the installed session: the one new ops are stamped with
    /// and the only one buffered events are accepted for.
    generation: u64,
}

/// The single owner of op correlation (and cancel bookkeeping). Internally
/// `Mutex`-guarded so the bridge's worker threads and the pump thread share it.
pub struct OpRegistry {
    inner: Mutex<Inner>,
}

impl OpRegistry {
    pub fn new() -> Arc<OpRegistry> {
        Arc::new(OpRegistry {
            inner: Mutex::new(Inner {
                ops: HashMap::new(),
                pending: HashMap::new(),
                generation: FIRST_GENERATION,
            }),
        })
    }

    /// The installed generation, to stamp a START with: read before the op is
    /// started and handed back to [`OpRegistry::register`], which is where the two
    /// are compared.
    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    /// Record an op started under `generation`, or hand it back if a swap has
    /// installed another session since - the check and the insert are one step, so
    /// a swap either sees the op (and drains it) or is seen by it.
    ///
    /// A session's ops are its own, so the op is stamped with the generation it was
    /// STARTED under rather than with whatever is installed by the time it is
    /// recorded; those differ exactly when it can no longer be recorded at all.
    pub fn register(&self, generation: u64, op_id: u64, entry: OpEntry) -> Registered {
        let mut inner = self.lock();
        if generation != inner.generation {
            return Registered::Orphaned(entry);
        }
        inner.ops.insert(op_id, Slot { generation, entry });
        // Only the installed generation's events are buffered, so every replayed
        // event is one this op's own session produced.
        Registered::Replay(inner.pending.remove(&op_id).unwrap_or_default())
    }

    /// Buffer an event whose op is not yet registered (the pump calls this on a
    /// miss); [`OpRegistry::register`] later returns it. An event from a session
    /// that is not the installed one is DROPPED instead: its op was ended by the
    /// [`OpRegistry::drain_and_install`] that swapped that session out, and
    /// buffering it would hand it to whichever op takes the id next.
    pub fn buffer(&self, generation: u64, op_id: u64, event_json: String) {
        let mut inner = self.lock();
        if generation != inner.generation {
            return;
        }
        inner
            .pending
            .entry(op_id)
            .or_default()
            .push((generation, event_json));
    }

    /// Snapshot the [`AsyncKind`] of an op registered under `generation` without
    /// removing it (a progress event does not end the op). `None` means no op of
    /// that session holds the id (the caller buffers). `AsyncKind` is `Copy` -
    /// every field is a function pointer - so this clones nothing of the entry.
    pub fn kind(&self, generation: u64, op_id: u64) -> Option<AsyncKind> {
        self.lock()
            .ops
            .get(&op_id)
            .filter(|slot| slot.generation == generation)
            .map(|slot| slot.entry.kind)
    }

    /// Remove and return an op registered under `generation` (a terminal ends it
    /// exactly once - whoever wins the take owns the terminal, so a rare concurrent
    /// dispatch cannot double-emit). `None` means no op of that session holds the
    /// id (the caller buffers).
    pub fn take(&self, generation: u64, op_id: u64) -> Option<OpEntry> {
        let mut inner = self.lock();
        if inner.ops.get(&op_id)?.generation != generation {
            return None;
        }
        inner.ops.remove(&op_id).map(|slot| slot.entry)
    }

    /// Find the in-flight op for `command` and signal its cancellation, returning
    /// whether one was found and signaled (the `cancelUpdate` path, which targets
    /// the single in-flight `startUpdate`). Sound only where the core admits ONE op
    /// of that command at a time; where several can run, key on the mod as well
    /// ([`OpRegistry::cancel_by_command_and_mod`]).
    pub fn cancel_by_command(&self, command: &str) -> bool {
        self.cancel_first(|entry| entry.command == command)
    }

    /// Find the in-flight op for `command` whose captured context names `mod_id`
    /// and signal its cancellation, returning whether one was found and signaled
    /// (the `cancelInstallMod` / `cancelCompileMod` path). Per-mod ops run
    /// concurrently - two cards can install two different mods - so the command
    /// alone does not identify one; the context's `modId`, the id the starting
    /// request carried, does.
    pub fn cancel_by_command_and_mod(&self, command: &str, mod_id: &str) -> bool {
        self.cancel_first(|entry| {
            entry.command == command
                && entry.context.get("modId").and_then(Value::as_str) == Some(mod_id)
        })
    }

    /// Signal the first registered op `accept` matches. The op stays registered:
    /// its CANCELED terminal still produces the command's reply. The handle is
    /// cloned out under the lock and invoked outside it - a cancel runs the
    /// token's hooks on the calling thread, which must not happen under this one.
    fn cancel_first(&self, accept: impl Fn(&OpEntry) -> bool) -> bool {
        let handle = {
            let inner = self.lock();
            inner
                .ops
                .values()
                .find(|slot| accept(&slot.entry))
                .and_then(|slot| slot.entry.cancel.clone())
        };
        handle.map(|h| h.cancel()).unwrap_or(false)
    }

    /// Hand the registry over to the session carrying `generation`: remove every
    /// registered op and return it (the caller runs each one's terminal path, since
    /// an op-id is meaningful only to the session that issued it), drop every
    /// buffered event, and install the incoming generation.
    ///
    /// The three are ONE operation on purpose. Leaving `pending` behind would
    /// replay a drained op's events into the next op that takes its id, and
    /// leaving the generation behind would let the outgoing session's late events
    /// reach the incoming session's ops.
    ///
    /// `generation` is the incoming SESSION's, supplied rather than derived: the
    /// UI can return to a session it left (the local one it keeps for the process
    /// lifetime), which arrives carrying the generation it always had. The module
    /// docs cover why re-installing one is safe.
    pub fn drain_and_install(&self, generation: u64) -> Vec<(u64, OpEntry)> {
        let mut inner = self.lock();
        inner.pending.clear();
        inner.generation = generation;
        inner
            .ops
            .drain()
            .map(|(op_id, slot)| (op_id, slot.entry))
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use serde_json::json;

    use super::*;
    use crate::ipc::outcome::Terminal;
    use crate::pump::test_support::register;

    /// The terminal shaper the registered ops carry. These tests never route a
    /// terminal event, so it only has to exist.
    fn unused_shaper(
        _outcome: Result<Value, windhawk_core_host::HostError>,
        _context: &Value,
    ) -> Value {
        Value::Null
    }

    /// A cancel handle that records that it was signaled, standing in for the
    /// session-bound one (which needs a real core session to construct).
    struct RecordingCancel(AtomicBool);

    impl CancelHandle for RecordingCancel {
        fn cancel(&self) -> bool {
            self.0.store(true, Ordering::Release);
            true
        }
    }

    /// Register an op for `command` against `mod_id`, in the shape the install and
    /// compile handlers start it (the context carries the mod id), and hand back its
    /// cancel handle so the test can see which one was signaled.
    fn register_mod_op(
        registry: &OpRegistry,
        op_id: u64,
        command: &str,
        mod_id: &str,
    ) -> Arc<RecordingCancel> {
        let cancel = Arc::new(RecordingCancel(AtomicBool::new(false)));
        register(
            registry,
            op_id,
            OpEntry {
                command: command.to_owned(),
                message_id: op_id as i64,
                kind: AsyncKind {
                    terminal: Terminal::Shaped(unused_shaper),
                    progress: None,
                    effect: None,
                },
                context: json!({ "modId": mod_id }),
                cancel: Some(cancel.clone()),
            },
        );
        cancel
    }

    #[test]
    fn cancel_by_command_and_mod_signals_only_the_named_mods_op() {
        let registry = OpRegistry::new();
        let alpha = register_mod_op(&registry, 1, "installMod", "alpha");
        let beta = register_mod_op(&registry, 2, "installMod", "beta");

        assert!(registry.cancel_by_command_and_mod("installMod", "beta"));
        assert!(beta.0.load(Ordering::Acquire));
        // The concurrent install of a DIFFERENT mod is untouched - the reason the
        // cancel is keyed on the mod rather than on the command alone.
        assert!(!alpha.0.load(Ordering::Acquire));
    }

    #[test]
    fn cancel_by_command_and_mod_distinguishes_install_from_compile() {
        let registry = OpRegistry::new();
        let install = register_mod_op(&registry, 1, "installMod", "alpha");

        // The same mod, the other command: nothing to cancel.
        assert!(!registry.cancel_by_command_and_mod("compileMod", "alpha"));
        assert!(!install.0.load(Ordering::Acquire));
    }

    #[test]
    fn cancel_with_nothing_in_flight_is_a_no_op() {
        let registry = OpRegistry::new();
        register_mod_op(&registry, 1, "installMod", "alpha");

        assert!(!registry.cancel_by_command_and_mod("installMod", "missing"));
    }

    /// A start straddling a swap: the generation is taken, the swap drains a
    /// registry the op is not in yet, and the registration arrives too late. It
    /// must be REFUSED - recording it under the incoming generation would leave an
    /// op no event can reach, since its own session's events carry the outgoing
    /// one.
    #[test]
    fn an_op_started_before_a_swap_is_handed_back_rather_than_recorded() {
        let registry = OpRegistry::new();
        let started_under = registry.generation();
        let cancel = Arc::new(RecordingCancel(AtomicBool::new(false)));
        let entry = OpEntry {
            command: "installMod".to_owned(),
            message_id: 3,
            kind: AsyncKind {
                terminal: Terminal::Shaped(unused_shaper),
                progress: None,
                effect: None,
            },
            context: json!({ "modId": "alpha" }),
            cancel: Some(cancel),
        };

        // The swap runs while the op is starting, so the drain that ends the
        // outgoing session's ops does not see it.
        assert!(registry.drain_and_install(started_under + 1).is_empty());

        let handed_back = match registry.register(started_under, 1, entry) {
            Registered::Orphaned(entry) => entry,
            Registered::Replay(_) => {
                panic!("the op was recorded against a session it never ran on")
            }
        };
        assert_eq!(handed_back.message_id, 3);

        // Under neither generation, so nothing can find it - and there is nothing
        // for a later cancel or a later drain to reach either.
        assert!(registry.take(started_under, 1).is_none());
        assert!(registry.take(started_under + 1, 1).is_none());
        assert!(!registry.cancel_by_command_and_mod("installMod", "alpha"));
    }
}
