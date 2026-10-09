// The Tauri IPC transport. It mirrors the VSCode webview model so the
// front-end's protocol engine (webviewIPC.ts) is unchanged: outbound envelopes
// are forwarded to the native windhawk-ui shell via a single fire-and-forget
// Tauri command (wh_ipc), and inbound envelopes (replies and events) arrive on
// the wh-ipc Tauri event channel and are re-injected into the window 'message'
// pipeline by initTauriBridge - exactly where the VSCode host posts them - so
// the reply correlation and the event hooks are reused verbatim.
//
// The Tauri API is reached through the `withGlobalTauri` global rather than the
// @tauri-apps/api package, so this module pulls no new dependency into the
// shared front-end and the VSCode/website bundles are unaffected.

type TauriEvent<T> = { payload: T };

// Removes a previously registered event listener.
export type UnlistenFn = () => void;

type TauriGlobal = {
  core: {
    invoke: <T = unknown>(
      command: string,
      args?: Record<string, unknown>
    ) => Promise<T>;
  };
  event: {
    listen: <T = unknown>(
      event: string,
      handler: (event: TauriEvent<T>) => void
    ) => Promise<UnlistenFn>;
  };
};

declare global {
  interface Window {
    __TAURI__?: TauriGlobal;
    // The windhawk:// link a cold start was launched with, set by the shell's
    // initialization script before this bundle runs (takeInitialDeepLink).
    __WINDHAWK_DEEP_LINK__?: unknown;
  }
}

declare const WEBPACK_HAS_MOCKS: boolean;

const notAvailable = () => {
  throw new Error(
    'getState/setState are not available under the Tauri transport'
  );
};

// Same shape as vsCodeApi (getState/setState/postMessage); only postMessage is
// used. getState/setState are unused in the codebase and throw if ever called.
const tauriTransport = {
  getState: notAvailable,
  setState: notAvailable,
  postMessage: (msg: unknown) => {
    // Fire-and-forget: the reply comes back out of band on the wh-ipc channel
    // (initTauriBridge), as in the VSCode webview model. A rejection means the
    // native shell is gone - nothing actionable - so the promise is discarded.
    void window.__TAURI__?.core.invoke('wh_ipc', { envelope: msg });
  },
};

// Null with no shell to answer it, as vsCodeApi is null outside a VSCode webview.
// Builds that carry no mocks keep the transport unconditional, so the shipping
// window can never resolve itself to fixtures.
const tauriApi = WEBPACK_HAS_MOCKS && !window.__TAURI__ ? null : tauriTransport;

export default tauriApi;

// Register the inbound bridge once at startup: re-inject every wh-ipc envelope
// (replies and events) into the window 'message' pipeline the front-end already
// listens on, so webviewIPC.ts needs no Tauri-specific code.
export function initTauriBridge() {
  void window.__TAURI__?.event.listen('wh-ipc', (event) => {
    window.postMessage(event.payload, '*');
  });
}

// The Windhawk debug-log stream. Unlike the envelope bridge above, the native
// shell delivers these on their own raw Tauri channels (out of band from wh_ipc):
// the log volume is high, so the lines bypass the message pipeline and go straight
// to the log pane. These helpers are the only place the log pane touches
// window.__TAURI__, keeping raw-Tauri access confined to this module. The
// undefined they return with no __TAURI__ is the browser preview, where the pane
// mounts but nothing ever reveals it.

// Live captured lines arrive as batches (the shell coalesces a flood into arrays).
export function listenLogLines(
  handler: (lines: string[]) => void
): Promise<UnlistenFn> | undefined {
  return window.__TAURI__?.event.listen<string[]>('wh-log', (event) => {
    handler(event.payload);
  });
}

// The shell asks the pane to reveal itself (the show-log affordance and the
// compiler-output surface for a failed local compile).
export function listenLogShow(
  handler: () => void
): Promise<UnlistenFn> | undefined {
  return window.__TAURI__?.event.listen('wh-log-show', () => {
    handler();
  });
}

// The retained tail, requested once when the pane is first revealed to render the
// backlog before subscribing to the live stream.
export async function fetchLogBacklog(): Promise<string[]> {
  const lines = await window.__TAURI__?.core.invoke<string[]>('wh_log_backlog');
  return lines ?? [];
}

// Release the single-owner DBWIN capture when the pane is closed: capture is
// scoped to while the pane is open.
export function stopLogCapture(): void {
  void window.__TAURI__?.core.invoke('wh_log_stop_capture');
}

// A windhawk:// link, as the shell parsed it out of a launch's command line
// (`windhawk://mods/<id>` opens that mod in the online browser). The shell has
// already checked the id against the mod-id grammar; the page only ever puts it
// in a route. Two deliveries, one shape: a launch that started the app leaves
// the link in a global before this bundle runs (the cold path), and a launch
// forwarded to the running app arrives as a raw Tauri event (the warm path).
export type DeepLink = { kind: 'mod'; modId: string };

function isDeepLink(value: unknown): value is DeepLink {
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as { kind?: unknown }).kind === 'mod' &&
    typeof (value as { modId?: unknown }).modId === 'string'
  );
}

// The link the app was started with, if any, read before the router is created
// so the first page is the mod's. Clearing it is what keeps a second document
// load (there is none in the ordinary run) from replaying it.
export function takeInitialDeepLink(): DeepLink | null {
  const value = window.__WINDHAWK_DEEP_LINK__;
  delete window.__WINDHAWK_DEEP_LINK__;
  return isDeepLink(value) ? value : null;
}

// A link forwarded by a launch while the app is running; the shell has already
// brought the window to the front.
export function listenDeepLink(
  handler: (link: DeepLink) => void
): Promise<UnlistenFn> | undefined {
  return window.__TAURI__?.event.listen<DeepLink>('wh-deep-link', (event) => {
    handler(event.payload);
  });
}
