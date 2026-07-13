//! Raw C ABI transport for `windhawk-core.dll`: the low-level client shared by
//! the Node bridge and the native CLI. It loads the DLL, resolves the exports,
//! hard-gates the ABI integer, owns the session lifecycle and the callback
//! trampolines, and marshals strings - and NOTHING else. No Windhawk semantics:
//! no command names, no DTO knowledge, no error mapping, no JSON of its own (it
//! returns raw envelope strings). Like the bridge it sits OUTSIDE the core
//! layering, reaching the core only through the C ABI, and depends on
//! `libloading` ALONE.
//!
//! Consumer policy that deliberately stays OUT of this crate:
//! - the `{ok,result}` / `{ok:false,error}` envelope split and error mapping -
//!   `invoke` returns the RAW envelope string so the bridge can forward it to
//!   its TS client unparsed;
//! - the `contractVersion` check - only the ABI INTEGER is hard-gated here, so
//!   the bridge keeps its TS-side graceful fallback (`get_info_json` is raw);
//! - the event-consumption strategy (mpsc vs threadsafe-function) - the
//!   consumer supplies `Send` closures and core-client just delivers to them.

#![deny(unsafe_op_in_unsafe_fn)]

mod api;
mod error;
mod loader;
mod session;

pub use error::{ClientError, ClientErrorKind};
pub use loader::{CoreLibrary, EXPECTED_ABI_VERSION};
pub use session::{CoreSession, SessionCallbacks};
