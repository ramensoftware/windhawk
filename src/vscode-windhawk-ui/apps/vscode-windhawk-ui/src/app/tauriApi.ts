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
// shared front-end and the extension/website bundles are unaffected.

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
  }
}

const notAvailable = () => {
  throw new Error(
    'getState/setState are not available under the Tauri transport'
  );
};

// Same shape as vsCodeApi (getState/setState/postMessage); only postMessage is
// used. getState/setState are unused in the codebase and throw if ever called.
const tauriApi = {
  getState: notAvailable,
  setState: notAvailable,
  postMessage: (msg: unknown) => {
    // Fire-and-forget: the reply comes back out of band on the wh-ipc channel
    // (initTauriBridge), as in the VSCode webview model. A rejection means the
    // native shell is gone - nothing actionable - so the promise is discarded.
    void window.__TAURI__?.core.invoke('wh_ipc', { envelope: msg });
  },
};

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
// window.__TAURI__, keeping raw-Tauri access confined to this module. They are
// used only by the Tauri-only log pane, so the returned undefined (no __TAURI__)
// never happens in practice - it just keeps the types honest for other builds.

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

// Release the single-owner DBWIN capture when the pane is closed (R7: capture is
// scoped to while the pane is open).
export function stopLogCapture(): void {
  void window.__TAURI__?.core.invoke('wh_log_stop_capture');
}
