//! The async op pump: the op-id registry, the generic event dispatcher, the
//! profile watcher, and the startup catalog refresh. The bridge starts ops and
//! records them in [`ops::OpRegistry`]; the core's event callback feeds
//! `(op_id, event_json)` to [`events::dispatch_event`], which routes each event
//! to the op's registered handling. Per-command knowledge lives in the
//! `commands/` handler that built the `AsyncKind`, never here.

pub mod events;
pub mod ops;
pub mod profile_watch;
pub mod startup;

#[cfg(test)]
pub mod test_support;
