// The webview IPC contract is single-sourced in the @windhawk/webview-ipc-contract
// package (shared verbatim with the windhawk-frontend front end and mirrored by the
// Rust host). This module re-exports it so existing './webviewIPCMessages' imports keep
// resolving; add or change message types in the package, not here.
//
// The shared data shapes (ModConfig, AppSettings, ...) come through this contract too,
// but the DLL-facing code keeps importing them from ./coreClient/contract (the core
// contract's own source of truth); the two definitions are kept compatible.
export * from '@windhawk/webview-ipc-contract';
