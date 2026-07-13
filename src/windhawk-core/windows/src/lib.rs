//! Win32 (and OS) implementations of the windhawk-core ports. Knows nothing
//! about commands or services. This crate and `windhawk-core-ffi` are the
//! only two places
//! `unsafe` is permitted in the workspace; every block requires a
//! `// SAFETY:` comment.
//!
//! The crate provides the `Clock` port; the registry and INI (Win32 profile
//! API) `SettingsBackend` adapters, the storage resolver + installer-language
//! write, and the capturing `Processes` adapter; the `Files` adapter (atomic
//! replace via `MoveFileExW`) and the `NamedLock` adapter (a named Win32 mutex
//! for the profile read-modify-write); the WinHTTP `Http` adapter and the
//! detached `Processes` form (the NSIS installer launch); and the job-object
//! process form.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
#![deny(unsafe_op_in_unsafe_fn)]

mod clock;
mod files;
mod http;
mod ini;
mod named_lock;
mod os;
mod processes;
mod registry;
mod storage;
mod wide;

pub use clock::SystemClock;
pub use files::WindowsFiles;
pub use http::WindowsHttp;
pub use ini::IniBackend;
pub use named_lock::WindowsNamedLock;
pub use processes::RealProcesses;
pub use registry::RegistryBackend;
pub use storage::WindowsStorageProvider;
