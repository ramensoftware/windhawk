// The webview IPC contract is single-sourced in the @windhawk/webview-ipc-contract
// package (shared verbatim with the VSCode extension and mirrored by the Rust host).
// This module re-exports it so existing '@app/webviewIPCMessages' imports keep
// resolving; add or change message types in the package, not here.
export * from '@windhawk/webview-ipc-contract';
