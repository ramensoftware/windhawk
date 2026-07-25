// The unified feedback surface. Two antd surfaces, both reachable from
// non-React modules (the IPC layer) and registered once from the
// FeedbackSurface component so they consume the app's ConfigProvider (theme,
// direction) rather than the static antd API:
//
//  - notification: a command FAILURE the IPC layer surfaces (the `error` object
//    a reply carries on failure), shown by `surfaceWireError`.
//  - message: a transient client-side validation toast (e.g. invalid
//    JSON/YAML), shown by `showErrorMessage` / `showInfoMessage` from the
//    components.
//
// Kept transport-agnostic so the one interception point in webviewIPC.ts serves
// both the VSCode webview and the native Tauri shell.

import { message as staticMessage } from 'antd';
import type { ReactNode } from 'react';

import type { WireError } from '@windhawk/webview-ipc-contract';

// WireError (the machine-readable error a reply carries on a command failure) is part
// of the shared webview IPC contract; re-export it from the single source so existing
// importers from this module keep working.
export type { WireError };

/**
 * Codes that must NOT auto-surface a notification:
 * - CANCELED: a cancellation, not a failure to report.
 * - MOD_NOT_INSTALLED / MOD_NOT_IN_REPO: an expected absence during normal
 *   reads/browse, not an error.
 * - COMPILER_FAILED: already surfaced in the log window (the compiler-output panel),
 *   so surfacing it here too would double up.
 * - DEV_TOOLS_MISSING: importUserData fail-fasts with this when a local compile is
 *   needed but the development tools are missing; the import dialog raises the
 *   install-dev-tools prompt instead of a notification.
 */
export const AUTO_SURFACE_SKIP = new Set<string>([
  'CANCELED',
  'MOD_NOT_INSTALLED',
  'MOD_NOT_IN_REPO',
  'COMPILER_FAILED',
  'DEV_TOOLS_MISSING',
]);

/**
 * Whether a reply's `error` field is the standard error OBJECT (vs `startUpdate`'s
 * `error` STRING, which the update modal renders itself - this discriminator leaves
 * it for that flow).
 */
export function isWireError(value: unknown): value is WireError {
  if (!value || typeof value !== 'object') {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate['code'] === 'string' &&
    typeof candidate['message'] === 'string'
  );
}

// ---- notification: backend command failures ----

// A single reporter, registered by the FeedbackSurface component once the antd
// notification instance is available. The IPC layer is a plain module - not a React
// component - so it reaches the context-aware surface through this seam.
type Reporter = (error: WireError) => void;
let reporter: Reporter | null = null;

export function registerErrorReporter(fn: Reporter | null) {
  reporter = fn;
}

/**
 * Surface a command failure through the shared antd notification, unless its code is
 * in the skip set or no reporter is registered yet (e.g. website mode, or before the
 * surface mounts).
 */
export function surfaceWireError(error: WireError) {
  if (AUTO_SURFACE_SKIP.has(error.code)) {
    return;
  }
  reporter?.(error);
}

// ---- message: client-side validation toasts ----

type MessageInstance = ReturnType<typeof staticMessage.useMessage>[0];
let messageApi: MessageInstance | null = null;

export function registerMessageApi(api: MessageInstance | null) {
  messageApi = api;
}

// Fall back to the static API if the holder is not mounted yet, so a toast is never
// silently dropped (it just misses the ConfigProvider context in that rare window).
export function showErrorMessage(content: ReactNode) {
  (messageApi ?? staticMessage).error(content);
}

export function showInfoMessage(content: ReactNode, duration?: number) {
  (messageApi ?? staticMessage).info(content, duration);
}
