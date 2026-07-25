"use strict";
// The webview IPC contract: the postMessage protocol between the Windhawk React
// webview and its hosts (the VSCode extension and the Tauri native shell). This
// package is the single source of truth for that protocol; both TypeScript hosts
// import it, and the Rust host mirrors it with typed serde structs proven against
// the shared fixture corpus in ./fixtures.
//
// The webview is the superset consumer (it sees every host), so this file models
// the union of all hosts' messages; a given host implements the subset it needs.
Object.defineProperty(exports, "__esModule", { value: true });
exports.WEBVIEW_IPC_CONTRACT_VERSION = void 0;
// The contract version, asserted on the getInitialAppSettings handshake so a host
// shipped against a different contract fails loudly instead of mis-handling a
// message. Kept in lockstep with contract-version.json (a package test asserts
// equality; the Rust host reads that JSON to check its own constant).
exports.WEBVIEW_IPC_CONTRACT_VERSION = '1.2.0';
