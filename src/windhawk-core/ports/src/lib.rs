//! The effect traits (ports) of windhawk-core, plus `CancelToken` and the small
//! data types port signatures need. Traits only; production implementations
//! live in `windhawk-core-windows`.
//!
//! The ports: `CancelToken` and the `Clock` port for the runtime; the
//! `SettingsBackend` keyed value store and a `StorageProvider` for
//! `windhawk.ini` resolution (`storage/paths.ts`); the `Files` port and the
//! `NamedLock` port (the profile read-modify-write mutex); the `Http` port
//! (streaming GET with progress and cancellation); the `Processes` port in
//! three forms - the capturing form for `schtasks.exe`, the detached form for
//! the NSIS installer launch, and the job-object kill-on-cancel form the
//! compiler needs; the `Fonts` port (the installed font families behind
//! `listFontFamilies`); the `HotkeyCapture` port (one keyboard chord
//! recorded behind `captureHotkey`); and the `UserAccounts` port (the account
//! a session acts for, which keys a mod's user configuration).

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod accounts;
mod cancel;
mod clock;
mod files;
mod fonts;
mod hotkey_capture;
mod http;
mod named_lock;
mod os_error;
mod processes;
mod settings;
mod storage;

pub use accounts::UserAccounts;
pub use cancel::CancelToken;
pub use clock::Clock;
pub use files::{DirEntry, FileError, FileErrorKind, Files};
pub use fonts::Fonts;
pub use hotkey_capture::{CancelReason, CaptureOutcome, HotkeyCapture, HotkeyModifiers};
pub use http::{Http, HttpError, HttpRequest, HttpResponse, HttpSink};
pub use named_lock::{NamedLock, NamedLockGuard};
pub use os_error::{OsError, os_message};
pub use processes::{DetachedRequest, ProcessError, ProcessOutput, ProcessRequest, Processes};
pub use settings::{
    SettingsBackend, SettingsError, SettingsErrorKind, SettingsTree, TreeLocation, TreeValue,
};
pub use storage::{
    InstallerLanguage, ResolvedStorage, StorageInfo, StorageProvider, StorageResolveError,
};
