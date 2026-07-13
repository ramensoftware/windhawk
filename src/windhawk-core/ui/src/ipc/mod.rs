//! The IPC layer: the webview envelope protocol, the emit seam, reply shaping,
//! the handler outcome vocabulary, the total dispatch, and the `wh_ipc` bridge
//! that drives them.

pub mod bridge;
pub mod dispatch;
pub mod emit_sink;
pub mod envelope;
pub mod outcome;
pub mod reply;
