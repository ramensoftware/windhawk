//! [`Outcome`] and the async vocabulary: what a command handler hands back to
//! the bridge, and the per-command knowledge an async op carries so the pump
//! can turn its terminal/progress events into the command's reply/events.
//!
//! The vocabulary is deliberately concentrated here as one tight type family: a
//! started op carries a [`Terminal`] (how its terminal becomes the one reply)
//! and an optional [`ProgressMapper`] (how its progress events become event
//! envelopes). The "exactly one reply" invariant is structural - `progress`
//! returns events only and is never handed the terminal, `records` yields a call
//! and never a reply, and `terminal` always yields exactly one reply - so no
//! [`AsyncKind`] can carry two reply sources or none. The shapers/mappers are pure `fn` pointers; the per-op state (the
//! originating correlation and any captured context) lives in the
//! [`OpRegistry`](crate::pump::ops::OpRegistry), not in the function.

use std::sync::Arc;

use serde_json::Value;
use windhawk_core_host::{CancelHandle, HostError};
use windhawk_core_protocol::OperationEvent;

use crate::ipc::envelope::Envelope;

/// A handler's result for one inbound envelope.
pub enum Outcome {
    /// A synchronous `messageWithReply` answered inline. The `Value` is the reply
    /// `data`, already shaped to the command's reply contract; the bridge writes it
    /// to the [`EmitSink`](crate::ipc::emit_sink::EmitSink) as a `reply` envelope.
    Reply(Value),
    /// An asynchronous command that has been started (`WhCoreInvokeAsync`). The
    /// op's terminal event becomes the `reply` later (and, for `startUpdate`, its
    /// progress events become `event`s); no reply is emitted now. The bridge
    /// records the op in the registry so the pump can route its events.
    Async(AsyncOp),
    /// A fire-and-forget `message` with no reply (`showLogOutput` and the
    /// `message`-type development stubs have no `messageId` to answer, so they only
    /// act/log).
    Done,
}

/// What one [`crate::ipc::bridge::BridgeCtx::start_async`] produced: everything
/// about the op that comes from the session rather than from the command.
///
/// The `generation` is the one belonging to the session that issued `op_id`,
/// taken from the swap point in the same read, rather than whichever is installed
/// at registration. That is what pins the op to the session that ran it: a swap
/// landing anywhere in the start window is then visible to the registry, which
/// ends the op instead of recording it against a session that never issued its id
/// ([`crate::pump::ops::OpRegistry::register`]).
pub struct Started {
    pub op_id: u64,
    pub generation: u64,
    pub cancel: Arc<dyn CancelHandle>,
}

/// A started asynchronous operation handed back from a handler: the [`Started`]
/// op, the per-command [`AsyncKind`], and the captured per-op `context` the
/// terminal shaper / composite reads (e.g. `installMod`'s pre-parsed metadata, or
/// `getRepositoryModSourceData`'s `modId`/`version`). `Value::Null` when none is
/// needed.
pub struct AsyncOp {
    pub start: Started,
    pub kind: AsyncKind,
    pub context: Value,
}

/// A pure mapper from a progress [`OperationEvent`] to the `event` envelopes it
/// produces. It is never handed the terminal event, so it structurally cannot
/// produce a reply. `Some` for the commands that stream progress (`startUpdate`
/// and the user-data import).
pub type ProgressMapper = fn(&OperationEvent) -> Vec<Envelope>;

/// A pure mapper from a progress [`OperationEvent`] to the [`HostEffect`] it calls
/// for, if any. Like [`ProgressMapper`] it never sees the terminal event, so an
/// effect can neither stand in for nor race the op's one reply.
pub type EffectMapper = fn(&OperationEvent) -> Option<HostEffect>;

/// Host-owned state an op changes behind the front-end's back, which the bridge
/// announces on its behalf. The dispatcher only NAMES the effect; the bridge -
/// which holds the context - carries it out, so the routing stays ctx-free and
/// headless-testable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostEffect {
    /// The app settings were written by something other than `updateAppSettings`
    /// (a user-data import applying the archive's settings), so the front-end's
    /// `appUISettings` - the language and the theme among them - and the native
    /// window/editor theme show the old values until they are re-announced.
    AppSettingsChanged,
}

/// A pure mapper from an async op's terminal outcome (a success `result` `Value`
/// or a [`HostError`]) plus the op's captured `context` into the command's reply
/// `Value`. Success and failure are the two branches of ONE function, so a
/// command's reply representation cannot drift between them.
pub type TerminalShaper = fn(Result<Value, HostError>, &Value) -> Value;

/// Builds the one call an op owes the machine on a SUCCESSFUL terminal, beside
/// whatever it replies: the completed value and the context in, the write out.
/// Pure, like every other mapper here - the call itself is made through the
/// follow-up seam.
pub type TerminalRecord = fn(&Value, &Value) -> FollowUp;

/// What a started async op produces and how. A plain, cheap value: `Copy`
/// (every field is a `fn` pointer or `Option<fn>`), snapshotted out of the
/// registry for progress without cloning the whole entry.
#[derive(Clone, Copy)]
pub struct AsyncKind {
    pub terminal: Terminal,
    /// `Some` only for the commands that stream progress; the common case is
    /// `None`.
    pub progress: Option<ProgressMapper>,
    /// The [`HostEffect`]s this op's progress asks the bridge for, beyond the
    /// envelopes `progress` produces. `Some` only for the user-data import,
    /// whose app-settings step changes state the front-end holds.
    pub effect: Option<EffectMapper>,
    /// A write this op's result implies, which its reply does not carry: the two
    /// catalog fetches record the versions they fetched in the user profile. It
    /// runs on a successful terminal only, through the same seam a composite's
    /// follow-up uses and BEFORE it, and its result is discarded - a write is
    /// not a term of the reply. A failure is LOGGED and the reply stands: the
    /// caller asked for a catalog, not for the write, and the profile converges
    /// on the next fetch or listing either way.
    pub records: Option<TerminalRecord>,
}

/// How an async op's TERMINAL event becomes the one reply.
#[derive(Clone, Copy)]
pub enum Terminal {
    /// The reply is the command's shaper applied to the terminal outcome (the same
    /// shaper the handler applies on a synchronous start failure, so success and
    /// failure shaping are single-sourced).
    Shaped(TerminalShaper),
    /// The reply needs more than the op's terminal value, so it pairs it with ONE
    /// follow-up core call. Either the reply IS that call's result, the op having
    /// fetched what it is made from (`getRepositoryMods`,
    /// `getRepositoryModSourceData`), or the op answers for itself and the call
    /// names the fields its result does not (`installMod`, `compileMod`, which read
    /// the mod's entry after the operation). Which of the two a command is, is what
    /// [`Completion::on_follow_up_failure`] says.
    Composite(Completion),
    /// An internal background op with no front-end reply (the startup catalog
    /// refresh): the terminal runs a side-effecting handler through the follow-up
    /// seam and emits nothing.
    Internal(InternalTerminal),
}

/// A composite's reply, as pure pieces plus its failure shapers. The follow-up
/// CALL between `follow_up` and `merge` is the one impure step; the pump reaches it
/// through an injected seam, so the routing is headless-testable.
#[derive(Clone, Copy)]
pub struct Completion {
    /// Build the one follow-up call from the completed value + context. It
    /// constructs the typed request DTO and erases it to a [`FollowUp`],
    /// keeping `Completion` monomorphic across the composites.
    pub follow_up: fn(&Value, &Value) -> FollowUp,
    /// Merge the completed value, the follow-up result, and the context into the
    /// reply `Value` (a pure `shape/` shaper).
    pub merge: fn(&Value, &Value, &Value) -> Value,
    /// The failure reply (a terminal `Failed`, or a follow-up that itself errors);
    /// `follow_up`/`merge` are not consulted. Pure over the context.
    pub on_failure: fn(&Value) -> Value,
    /// The reply when the op ITSELF succeeded and only its follow-up call failed.
    /// The completed value is in hand there, so a command whose reply is mostly its
    /// own terminal - an install, whose mod is on the machine whatever the read
    /// after it did - answers with what it has rather than reporting a failure of
    /// something the caller did not ask for. `None` is for the composites whose
    /// reply IS the follow-up: they have nothing to answer with, so the follow-up's
    /// failure is the command's and it routes through `on_failure`.
    ///
    /// The error rides the reply either way. This shaper picks what the reply
    /// REPORTS, not whether it admits a failure - a reply that reported a whole
    /// success would have the front-end adopt the stand-in values it carries for
    /// the fields the follow-up was to have named.
    pub on_follow_up_failure: Option<fn(&Value, &Value) -> Value>,
}

/// An internal background op's terminal handler: it acts via the follow-up seam
/// (so it can run any sync/stateless core call) and emits nothing. The startup
/// catalog refresh is the sole user.
pub type InternalTerminal =
    fn(Result<Value, HostError>, &Value, &dyn Fn(&FollowUp) -> Result<Value, HostError>);

/// A composite's (or internal op's) one follow-up call, erased to the command name
/// and already-serialized params the host invokes. `stateless` routes it to the
/// session-free `GatedCore` path (`parseModSource`) vs the `Session` (everything
/// else).
pub struct FollowUp {
    pub command: &'static str,
    pub params: Value,
    pub stateless: bool,
}
