//! In-memory port implementations and fault injection for windhawk-core
//! tests. Behavioral fakes, not expectation mocks: tests
//! assert on outcomes. Never linked into the shipping DLL (dev-dependency
//! only).

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod accounts;
mod clock;
mod files;
mod fonts;
mod hotkey_capture;
mod http;
mod named_lock;
mod processes;
mod settings;
mod storage;

pub use accounts::{FakeUserAccounts, TEST_USER_NAME, TEST_USER_SID};
pub use clock::FakeClock;
pub use files::FakeFiles;
pub use fonts::FakeFonts;
pub use hotkey_capture::{FakeCaptureEnd, FakeHotkeyCapture};
pub use http::{FakeHttp, FakeResponse};
pub use named_lock::FakeNamedLock;
pub use processes::FakeProcesses;
pub use settings::{FakeSettings, tree_key};
pub use storage::FakeStorageProvider;
