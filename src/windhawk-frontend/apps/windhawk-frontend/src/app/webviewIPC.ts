import { useCallback, useEffect, useRef, useState } from 'react';

import backendApi from './backendApi';
import { promptDevToolsInstall } from './devToolsInstall';
import { isWireError, surfaceWireError, type WireError } from './feedback';
import {
  WEBVIEW_IPC_CONTRACT_VERSION,
  isValidSuppression,
  type CancelCaptureHotkeyData,
  type CancelCaptureHotkeyReplyData,
  type CancelCompileModData,
  type CancelCompileModReplyData,
  type CancelInstallDevToolsReplyData,
  type CancelInstallModData,
  type CancelInstallModReplyData,
  type CancelUpdateReplyData,
  type CompileEditedModData,
  type CompileEditedModReplyData,
  type CancelImportUserDataReplyData,
  type CaptureHotkeyData,
  type CaptureHotkeyReplyData,
  type CompileModData,
  type CompileModReplyData,
  type DeleteEditedModReplyData,
  type DeleteModData,
  type DeleteModReplyData,
  type DeleteModUserConfigData,
  type DeleteModUserConfigReplyData,
  type DevActionReplyData,
  type DevToolsInstallDownloadProgressEventData,
  type DevToolsInstallingEventData,
  type EditModData,
  type EnableEditedModData,
  type EnableEditedModLoggingData,
  type EnableEditedModLoggingReplyData,
  type EnableEditedModReplyData,
  type EnableModData,
  type EnableModReplyData,
  type ExitEditorModeData,
  type ExitEditorModeReplyData,
  type ExportUserDataData,
  type ExportUserDataReplyData,
  type ForkModData,
  type GetAppSettingsReplyData,
  type GetFeaturedModsReplyData,
  type GetInitialAppSettingsReplyData,
  type GetInstalledModsReplyData,
  type GetModReviewVotesData,
  type GetModReviewVotesReplyData,
  type GetModConfigData,
  type GetModConfigReplyData,
  type GetModDynamicSelectOptionsData,
  type GetModDynamicSelectOptionsReplyData,
  type GetModSettingsData,
  type GetModSettingsReplyData,
  type GetModSourceDataData,
  type GetModSourceDataReplyData,
  type GetModVersionsData,
  type GetModVersionsReplyData,
  type GetRepositoryModSourceDataData,
  type GetRepositoryModSourceDataReplyData,
  type GetRepositoryModsReplyData,
  type HotkeyCaptureProgressEventData,
  type ImportUserDataData,
  type ImportUserDataProgressEventData,
  type ImportUserDataReplyData,
  type InspectUserDataData,
  type InspectUserDataReplyData,
  type InstallModData,
  type InstallModReplyData,
  type ListFontFamiliesData,
  type ListFontFamiliesReplyData,
  type NoData,
  type PickFilePathData,
  type PickFilePathReplyData,
  type RetractModReviewVoteData,
  type RetractModReviewVoteReplyData,
  type SetEditedModDetailsData,
  type SetEditedModIdData,
  type SetModSettingsData,
  type SetModSettingsReplyData,
  type SetModUserScopeData,
  type SetModUserScopeReplyData,
  type SetNewAppSettingsData,
  type SetNewModConfigData,
  type StartInstallDevToolsReplyData,
  type StartUpdateReplyData,
  type UpdateAppSettingsData,
  type UpdateAppSettingsReplyData,
  type UpdateDownloadProgressEventData,
  type UpdateInstalledModsDetailsData,
  type UpdateInstallingEventData,
  type UpdateModConfigData,
  type UpdateModConfigReplyData,
  type UpdateModRatingData,
  type UpdateModRatingReplyData,
  type VoteModReviewData,
  type VoteModReviewReplyData
} from './webviewIPCMessages';
/// #if HAS_MOCKS
import type { MockDataRegistry } from './mocking';
import {
  hostEventsBeforeReply,
  hostEventsWithReply,
  installedModDetailsAfterOperation,
  mockReplyDelayMs,
  mockUserScopeReply,
  repositoryModsListing,
  useMockContext,
  writeMockReply,
} from './mocking';
import { applyScenarioReply } from './mocking/mockScenarios';
/// #endif

// Use webpack constants for conditional compilation
declare const WEBPACK_IS_WEBSITE: boolean;
declare const WEBPACK_IS_TAURI: boolean;
declare const WEBPACK_HAS_MOCKS: boolean;

// Message types:
// * 'message' is a message from the webview to the extension.
// * 'messageWithReply' is a message from the webview to the extension that expects a reply.
// * 'reply' is a reply to a 'messageWithReply' message.
// * 'event' is a message from the extension to the webview.
type MessageType = 'message' | 'messageWithReply' | 'reply' | 'event';

type CommonMessageBase = {
  type: MessageType;
  command: string;
  data: Record<string, unknown>;
};

type MessageRegular = CommonMessageBase & {
  type: 'message';
  command: string;
  data: Record<string, unknown>;
};

type MessageWithReply = CommonMessageBase & {
  type: 'messageWithReply';
  command: string;
  data: Record<string, unknown>;
  messageId: number;
};

type Reply = CommonMessageBase & {
  type: 'reply';
  command: string;
  data: Record<string, unknown>;
  messageId: number;
};

type Event = CommonMessageBase & {
  type: 'event';
  command: string;
  data: Record<string, unknown>;
};

type MessageAny = MessageRegular | MessageWithReply | Reply | Event;

/**
 * Whether an inbound envelope came from a window that speaks for the host.
 *
 * Only the Tauri build can answer that. There the bridge re-injects what arrived
 * on the wh-ipc channel into this window (tauriApi.ts), so the host is this
 * window itself and anything else holding a handle on it - an opener, a frame the
 * app hosts - is not. The distinction is worth drawing because the envelope does
 * not draw it: message ids run in sequence from the first request of the session
 * and command names are public, so a reply another window fabricates correlates
 * against a request in flight.
 *
 * The VSCode webview posts from the frame around this one, and that frame cannot
 * be named here: the webview host deletes `window.parent`, `window.top` and
 * `window.frameElement` from the content frame before the app's code runs
 * (vs/workbench/contrib/webview/browser/pre/index.html), having first captured
 * the outbound post function. `window.parent` is therefore undefined and no
 * comparison identifies the sender, so the check stands down there and the
 * isolation rests on the webview CSP - `default-src 'none'`, under which no frame
 * the app hosts exists to post at all. Identifying the host by origin instead is
 * no help: the shell posts with the webview's own origin, which is this window's
 * origin too.
 *
 * The website build has no host and compiles the listeners out entirely, so it
 * never reaches this.
 */
function isFromHostWindow(event: MessageEvent) {
  return WEBPACK_IS_TAURI ? event.source === window : true;
}

/**
 * Show a failure the host has not already shown.
 *
 * Which side of the transport tells the user about a failure is the host's to
 * decide, and the object a reply carries serves the app either way. The Tauri host
 * shows nothing of its own, so there this notification is the report. The VSCode
 * extension pops a native notification from every catch it answers a request from,
 * so a notification here would say the same thing a second time in a second style,
 * and what it attaches is left to the app to act on - `uiMissing`, a listing that
 * came up short, read-back fields that are stand-ins.
 *
 * With no host at all (mock mode: the browser preview) nothing has told the user
 * anything, so the app shows the failure itself. The website build has
 * no host either, but sends nothing to fail and never reaches this.
 */
function surfaceUnreportedWireError(error: WireError) {
  if (WEBPACK_IS_TAURI || !backendApi) {
    surfaceWireError(error);
  }
}

////////////////////////////////////////////////////////////
// Messages.

// The launch entry points (createNewMod / editMod / forkMod) are `messageWithReply`s
// so the native UI can react. They are called as plain actions from many places, so
// each handles its own reply here rather than through a component hook: a standard
// error object goes to the same surfacing rule the reply hook applies to other
// commands, and a `uiMissing` reply opens the "install development tools" modal
// through the registered prompt seam. Success is a no-op.
// Resolves true when the dev action is opening the editor, and false when it is
// not - it failed, or dev tools are missing so the install modal was raised
// instead. Callers use this to drop any pending-launch UI when the editor will not
// appear.
function dispatchDevActionReply(
  promise: Promise<DevActionReplyData>
): Promise<boolean> {
  return promise
    .then((reply) => {
      if (isWireError(reply.error)) {
        surfaceUnreportedWireError(reply.error);
        return false;
      } else if (reply.uiMissing) {
        promptDevToolsInstall();
        return false;
      }
      return true;
    })
    .catch((error) => {
      // A cancelled request is an end rather than a failure; anything else rethrows.
      if (error instanceof MessageCancelledError) {
        return false;
      }
      throw error;
    });
}

function sendDevActionToHost(
  command: string,
  data: Record<string, unknown>
): Promise<DevActionReplyData> {
  return sendMessageWithReply<Record<string, unknown>, DevActionReplyData>(
    command,
    data
  ).promise;
}

/// #if HAS_MOCKS
/**
 * The launch round trip, answered by the mock host when nothing else will.
 *
 * These three post outside the command hooks, so the mock wrapper those are built
 * on does not cover them - and mock mode is exactly the state where `backendApi`
 * is null, so the envelope would reach nothing and the promise behind it would
 * never settle, leaving whatever waits on the launch waiting for good. The mock
 * host answers as it does for a hook: the echo on the window that makes the
 * request observable, then the reply a tick later through the same scenario
 * rewrite. An empty reply is the editor opening.
 */
function sendDevActionWithMock(
  command: string,
  data: Record<string, unknown>
): Promise<DevActionReplyData> {
  if (backendApi) {
    return sendDevActionToHost(command, data);
  }

  window.postMessage({ type: 'messageWithReply', command, data }, '*');
  return new Promise((resolve) =>
    setTimeout(
      () => resolve(applyScenarioReply<DevActionReplyData>(command, {})),
      0
    )
  );
}
/// #endif

const sendDevAction = WEBPACK_HAS_MOCKS
  ? sendDevActionWithMock
  : sendDevActionToHost;

export function createNewMod() {
  return dispatchDevActionReply(sendDevAction('createNewMod', {}));
}

export function editMod(data: EditModData) {
  return dispatchDevActionReply(sendDevAction('editMod', data));
}

export function forkMod(data: ForkModData) {
  return dispatchDevActionReply(sendDevAction('forkMod', data));
}

export function showAdvancedDebugLogOutput() {
  const msg: MessageRegular = {
    type: 'message',
    command: 'showAdvancedDebugLogOutput',
    data: {},
  };
  backendApi?.postMessage(msg);
}

export function showLogOutput() {
  const msg: MessageRegular = {
    type: 'message',
    command: 'showLogOutput',
    data: {},
  };
  backendApi?.postMessage(msg);
}

export function getInitialSidebarParams() {
  const msg: MessageRegular = {
    type: 'message',
    command: 'getInitialSidebarParams',
    data: {},
  };
  backendApi?.postMessage(msg);
}

export function stopCompileEditedMod() {
  const msg: MessageRegular = {
    type: 'message',
    command: 'stopCompileEditedMod',
    data: {},
  };
  backendApi?.postMessage(msg);
}

export function previewEditedMod() {
  const msg: MessageRegular = {
    type: 'message',
    command: 'previewEditedMod',
    data: {},
  };
  backendApi?.postMessage(msg);
}

////////////////////////////////////////////////////////////
// Messages with replies.

// Global message ID counter for generating unique message IDs.
let globalMessageId = 0;

/**
 * Error thrown when a message request is cancelled.
 */
class MessageCancelledError extends Error {
  constructor() {
    super('Message request was cancelled');
    this.name = 'MessageCancelledError';
  }
}

/**
 * Non-React async function that sends a message and waits for a reply.
 *
 * The request settles on the host's reply or on `cancel()`, and carries no
 * deadline of its own. The host is expected to answer every `messageWithReply`,
 * an unknown command and a failed handler included, so a request that never
 * settles is a host defect to fix there - and worth fixing, because `pending`
 * latches and the screens gating navigation on it stop letting the user leave.
 * A deadline here could not tell that apart from a command whose length is the
 * user's or the network's (a file dialog, a download, an install), and would
 * report work that did land as work that may not have.
 *
 * @param eventName - The command name
 * @param data - The message data
 * @returns An object with a promise that resolves with the reply data and a cancel function
 */
function sendMessageWithReply<
  TPostMessage extends Record<string, unknown>,
  TReply
>(
  eventName: string,
  data: TPostMessage
): { promise: Promise<TReply>; cancel: () => void } {
  // The same browser-mode guard the reply and event hooks carry, and the reason
  // it is a build-time constant rather than a runtime check: it keeps the window
  // 'message' listener below out of the website bundle. That build is an ordinary
  // page - no host to answer it, and none of the CSP isolation the VSCode and
  // Tauri hosts give the webview - so a listener there would correlate a reply
  // fabricated by an opener or an embedder against a pending request.
  if (WEBPACK_IS_WEBSITE) {
    throw new Error(
      `sendMessageWithReply("${eventName}") must not be called in browser mode`
    );
  }

  let handler: ((event: MessageEvent) => void) | null = null;
  let rejectFn: ((reason: Error) => void) | null = null;

  const promise = new Promise<TReply>((resolve, reject) => {
    rejectFn = reject;

    globalMessageId++;
    if (globalMessageId > 0x7fffffff) {
      globalMessageId = 1;
    }

    const currentMessageId = globalMessageId;

    const message: MessageWithReply = {
      type: 'messageWithReply',
      command: eventName,
      data,
      messageId: currentMessageId,
    };

    handler = (event: MessageEvent<MessageAny>) => {
      if (!isFromHostWindow(event)) {
        return;
      }

      const msgData = event.data;
      if (
        msgData.type === 'reply' &&
        msgData.command === eventName &&
        msgData.messageId === currentMessageId
      ) {
        if (handler) {
          window.removeEventListener('message', handler);
          handler = null;
        }
        rejectFn = null;
        resolve(msgData.data as TReply);
      }
    };

    window.addEventListener('message', handler);
    backendApi?.postMessage(message);
  });

  const cancel = () => {
    if (handler) {
      window.removeEventListener('message', handler);
      handler = null;
    }
    if (rejectFn) {
      rejectFn(new MessageCancelledError());
      rejectFn = null;
    }
  };

  return { promise, cancel };
}

/**
 * The effects a reply has on the app itself, before it reaches the caller.
 *
 * Every reply funnels through here, so both happen whether the caller reads the
 * result or ignores it:
 *
 * - A standard error OBJECT is surfaced once, unless the host showed it already
 *   (see surfaceUnreportedWireError). startUpdate's error STRING is left to its own
 *   modal.
 * - `uiMissing` says the development tools are not on the machine, so the command
 *   did not run and there is nothing to report as failed: the app offers to install
 *   them instead, the way the launch entry points do for the same flag.
 *
 * The reply goes on to the caller's promise afterwards either way. A uiMissing one
 * carries null details, which a caller that only applies details ignores, while one
 * sequencing its next install off the reply would otherwise wait forever.
 */
function applyReplyEffects<TReply>(reply: TReply) {
  const { error, uiMissing } = reply as {
    error?: unknown;
    uiMissing?: boolean;
  };
  if (isWireError(error)) {
    surfaceUnreportedWireError(error);
  }
  if (uiMissing) {
    promptDevToolsInstall();
  }
}

/**
 * How a request ended. A reply, or nothing - the hook unmounted with the request
 * still open, so no reply is coming and none is owed.
 *
 * Two arms for the two exits a request has, rather than a rejection: an abandoned
 * request is the expected end of one, and a React event handler that forgets its
 * `catch` would turn it into an unhandled rejection.
 */
export type RequestResult<TReply> =
  | { status: 'reply'; data: TReply }
  | { status: 'abandoned' };

/**
 * React hook wrapper for sendMessageWithReply that manages pending state.
 *
 * The caller gets its own reply back: `postMessage` resolves with the reply to the
 * request THAT call sent, so nothing downstream has to work out which request an
 * answer belongs to.
 *
 * One hook instance serves every target a screen acts on - a mods browser mounts a
 * single useEnableMod for its whole grid - so several requests can be in flight at
 * once. They all run to completion, each resolving the call that sent it, and
 * `pending` covers the whole set. Reply order says nothing about request order:
 * both hosts dispatch each envelope concurrently (windhawk-core's wh_ipc spawns a
 * worker per envelope; the VSCode extension does not await its handler), so a
 * reply arrives when its own work finishes, and a slow disable of one mod lands
 * after a fast disable of another.
 */
function usePostMessageWithReply<
  TPostMessage extends Record<string, unknown>,
  TReply
>(eventName: string) {
  if (WEBPACK_IS_WEBSITE) {
    throw new Error(
      `usePostMessageWithReply("${eventName}") must not be called in browser mode`
    );
  }

  const [pending, setPending] = useState(false);
  // The cancel function of every request still awaiting a reply, keyed by request
  // id, so unmount can abandon all of them and `pending` can track the set.
  const inFlightRef = useRef(new Map<number, () => void>());
  const requestIdRef = useRef(0);

  // Cleanup on unmount
  useEffect(() => {
    const inFlight = inFlightRef.current;
    return () => {
      for (const cancel of inFlight.values()) {
        cancel();
      }
      inFlight.clear();
    };
  }, []);

  const postMessage = useCallback(
    async (data: TPostMessage): Promise<RequestResult<TReply>> => {
      const currentRequestId = ++requestIdRef.current;

      setPending(true);

      const { promise, cancel } = sendMessageWithReply<TPostMessage, TReply>(
        eventName,
        data
      );
      const inFlight = inFlightRef.current;
      inFlight.set(currentRequestId, cancel);

      try {
        const reply = await promise;
        applyReplyEffects(reply);
        return { status: 'reply', data: reply };
      } catch (error) {
        // Cancellation is expected behavior: the hook unmounted with the request
        // still open, which is the caller's other exit rather than a failure.
        if (error instanceof MessageCancelledError) {
          return { status: 'abandoned' };
        }
        throw error;
      } finally {
        inFlight.delete(currentRequestId);
        if (inFlight.size === 0) {
          setPending(false);
        }
      }
    },
    [eventName]
  );

  return { postMessage, pending };
}

/// #if HAS_MOCKS
/**
 * A host event, handed to the app's listeners before this returns, as a message
 * from the window that speaks for the host (see isFromHostWindow).
 */
function dispatchMockHostEvent(event: {
  command: string;
  data: Record<string, unknown>;
}) {
  window.dispatchEvent(
    new MessageEvent('message', {
      data: { type: 'event', command: event.command, data: event.data },
      source: window,
    })
  );
}

/**
 * Wrapper for usePostMessageWithReply that adds automatic mock data support.
 * When running in development mode (without VSCode API), automatically returns mock data
 * instead of making IPC calls.
 *
 * @param eventName - The IPC event name
 * @param mockDataSelector - Function to extract mock data from MockDataRegistry and request (only used in mock mode)
 * @returns Same interface as usePostMessageWithReply
 */
function usePostMessageWithReplyWithMockDev<
  TPostMessage extends Record<string, unknown>,
  TReply
>(
  eventName: string,
  mockDataSelector: (mockData: MockDataRegistry, request: TPostMessage) => TReply
) {
  const { isMockMode, mockData } = useMockContext();

  // Always call the real hook to maintain hook order (even if we won't use it)
  const realResult = usePostMessageWithReply<TPostMessage, TReply>(eventName);

  // Mock mode: create a simulated IPC call (always create it to maintain hook order)
  const mockPostMessage = useCallback(
    (data: TPostMessage) => {
      // There is no host to hand the envelope to, so echo it on the window the way
      // the real transport hands it over. A page watching the app - the browser
      // preview's devtools, whatever automates the browser build - can then see
      // the exact request an action produces. The app itself ignores it: its
      // listeners take only 'reply' and 'event' envelopes.
      window.postMessage(
        { type: 'messageWithReply', command: eventName, data },
        '*'
      );
      // Simulate async behavior. A command that stands for something the user
      // does in the meantime (a hotkey capture) takes the time its fixture
      // says; everything else answers on the next task.
      const mockReply = applyScenarioReply(
        eventName,
        mockDataSelector(mockData, data)
      );
      writeMockReply(
        eventName,
        mockData,
        data,
        mockReply as Record<string, unknown>
      );
      const delay = mockReplyDelayMs(eventName, mockData);
      // What a host pushes while it works, ahead of its answer - the held
      // modifiers of a capture in flight - spread over the wait.
      const onTheWay = hostEventsBeforeReply(
        eventName,
        mockData,
        mockReply as Record<string, unknown>
      );
      onTheWay.forEach((event, index) => {
        setTimeout(
          () => dispatchMockHostEvent(event),
          (delay * (index + 1)) / (onTheWay.length + 1)
        );
      });
      return new Promise<RequestResult<TReply>>((resolve) => {
        setTimeout(() => {
          // What a host pushes as it answers - the echo of a write - ahead of
          // the answer, as both hosts send it.
          for (const event of hostEventsWithReply(
            eventName,
            mockData,
            data,
            mockReply as Record<string, unknown>
          )) {
            dispatchMockHostEvent(event);
          }
          // Through the same reply effects the host round trip runs, so a
          // scenario's failure reply reaches the app the way a real one does -
          // and with no host to have reported it, the notification is raised
          // here.
          applyReplyEffects(mockReply);
          // The caller's own answer: a consumer awaiting its request has to
          // settle here as it does against a host, or it hangs in the browser
          // preview.
          resolve({ status: 'reply', data: mockReply });
        }, delay);
      });
    },
    [eventName, mockData, mockDataSelector]
  );

  // If we're not in mock mode, use real IPC
  if (!isMockMode) {
    return realResult;
  }

  return {
    postMessage: mockPostMessage,
    pending: false,
  };
}
/// #else
/**
 * Production version: simply forwards to usePostMessageWithReply.
 * Mock data selector is ignored.
 */
function usePostMessageWithReplyWithMockProd<
  TPostMessage extends Record<string, unknown>,
  TReply
>(eventName: string, _mockDataSelector: unknown) {
  return usePostMessageWithReply<TPostMessage, TReply>(eventName);
}
/// #endif

/**
 * The base every command hook is built on, mock answer required.
 *
 * Including for the commands whose mock answer says nothing interesting: mock mode
 * is exactly the state where there is no transport, so a hook on
 * usePostMessageWithReply posts its envelope into nothing there. Its request then
 * reaches no answer at all - the screen waiting on the reply is dead for the
 * session, and a caller awaiting the request waits for good, in the one build with
 * no host to blame for it. The echo it posts is also the only way a request is
 * observable from outside the app in the browser build.
 */
const usePostMessageWithReplyWithMock = WEBPACK_HAS_MOCKS
  ? usePostMessageWithReplyWithMockDev
  : usePostMessageWithReplyWithMockProd;

export function useGetInitialAppSettings() {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({
      // The mock host is this same build, so it reports our own contract version.
      contractVersion: WEBVIEW_IPC_CONTRACT_VERSION,
      appUISettings: mockData.appUISettings,
      currentUser: mockData.currentUser,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    NoData,
    GetInitialAppSettingsReplyData
  >('getInitialAppSettings', selector);
  return {
    getInitialAppSettings: result.postMessage,
    getInitialAppSettingsPending: result.pending,
  };
}

export function useInstallMod() {
  // A mock install always succeeds, reporting the mod at the version of the
  // source it was handed - not at the one on offer, which would have installing
  // an older version report the newest - and with the config a fresh install
  // carries.
  const selector = useCallback(
    (mockData: MockDataRegistry, request: InstallModData) => ({
      modId: request.modId,
      installedModDetails: installedModDetailsAfterOperation(
        mockData,
        request.modId,
        mockData.modVersionSource(
          request.modId,
          mockData.modVersionOfSource(request.modSource)
        ).metadata,
        {
          ...mockData.newModConfig,
          disabled: !!request.disabled,
          loggingEnabled: !!request.loggingEnabled,
        }
      ),
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    InstallModData,
    InstallModReplyData
  >('installMod', selector);
  return {
    installMod: result.postMessage,
    installModPending: result.pending,
  };
}

export function useCompileMod() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: CompileModData) => ({
      modId: request.modId,
      compiledModDetails: installedModDetailsAfterOperation(
        mockData,
        request.modId,
        mockData.installedModSourceData(request.modId).metadata,
        { ...mockData.newModConfig }
      ),
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    CompileModData,
    CompileModReplyData
  >('compileMod', selector);
  return {
    compileMod: result.postMessage,
    compileModPending: result.pending,
  };
}

// Ask the host to stop an in-flight install. It names the mod, unlike
// useCancelUpdate and its siblings: the host runs one update, one dev-tools
// install and one import at a time, but an install per mod, so the command alone
// would not say which. The reply only acknowledges that an install was found and
// signaled; the install's own reply still arrives, with null details, and it is
// the one that ends the pending state.
export function useCancelInstallMod() {
  // A mock install is answered within a task, so a cancel that reaches this host
  // always names one that has already settled - the race the reply's `succeeded`
  // reports as false.
  const selector = useCallback(
    (mockData: MockDataRegistry, request: CancelInstallModData) => ({
      modId: request.modId,
      succeeded: false,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    CancelInstallModData,
    CancelInstallModReplyData
  >('cancelInstallMod', selector);
  return {
    cancelInstallMod: result.postMessage,
    cancelInstallModPending: result.pending,
  };
}

// The recompile twin of useCancelInstallMod.
export function useCancelCompileMod() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: CancelCompileModData) => ({
      modId: request.modId,
      succeeded: false,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    CancelCompileModData,
    CancelCompileModReplyData
  >('cancelCompileMod', selector);
  return {
    cancelCompileMod: result.postMessage,
    cancelCompileModPending: result.pending,
  };
}

export function useEnableMod() {
  // The mock host refuses to turn on a mod its stored config says is nuked, in
  // the core's words - whatever the screen asking believes the mod to be.
  const selector = useCallback(
    (mockData: MockDataRegistry, request: EnableModData) => {
      const nuked = mockData.modConfig[request.modId]?.nuked;
      if (request.enable && nuked) {
        return {
          modId: request.modId,
          enabled: true,
          succeeded: false,
          error: {
            code: 'MOD_NUKED',
            message: `mod '${request.modId}' was disabled by the Windhawk mod catalog: ${nuked}`,
          },
        };
      }
      return {
        modId: request.modId,
        enabled: request.enable,
        succeeded: true,
      };
    },
    []
  );
  const result = usePostMessageWithReplyWithMock<
    EnableModData,
    EnableModReplyData
  >('enableMod', selector);
  return {
    enableMod: result.postMessage,
    enableModPending: result.pending,
  };
}

export function useDeleteMod() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: DeleteModData) => ({
      modId: request.modId,
      succeeded: true,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    DeleteModData,
    DeleteModReplyData
  >('deleteMod', selector);
  return {
    deleteMod: result.postMessage,
    deleteModPending: result.pending,
  };
}

export function useUpdateModRating() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: UpdateModRatingData) => ({
      modId: request.modId,
      rating: request.rating,
      succeeded: true,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    UpdateModRatingData,
    UpdateModRatingReplyData
  >('updateModRating', selector);
  return {
    updateModRating: result.postMessage,
    updateModRatingPending: result.pending,
  };
}

// The votes the user cast on a mod's reviews, read when its reviews are shown.
// A host over a core from before the command answers with the error attached,
// which the reviews modal takes as no votes and no voting.
export function useGetModReviewVotes() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: GetModReviewVotesData) => ({
      modId: request.modId,
      votes: mockData.reviewVotes[request.modId] ?? [],
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    GetModReviewVotesData,
    GetModReviewVotesReplyData
  >('getModReviewVotes', selector);
  return {
    getModReviewVotes: result.postMessage,
    getModReviewVotesPending: result.pending,
  };
}

// An upvote on a review: the reply is the mod's whole list after the write,
// which replaces the caller's cache. The mock host writes it back into the
// registry as the real one writes the profile, so a read that follows finds
// it.
export function useVoteModReview() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: VoteModReviewData) => {
      const votes = mockData.reviewVotes[request.modId] ?? [];
      return {
        modId: request.modId,
        votes: votes.some((vote) => vote.reviewId === request.reviewId)
          ? votes
          : [
              ...votes,
              {
                reviewId: request.reviewId,
                timestamp: Math.floor(Date.now() / 1000),
              },
            ],
        succeeded: true,
      };
    },
    []
  );
  const result = usePostMessageWithReplyWithMock<
    VoteModReviewData,
    VoteModReviewReplyData
  >('voteModReview', selector);
  return {
    voteModReview: result.postMessage,
    voteModReviewPending: result.pending,
  };
}

// A vote taken back: the reply is the mod's whole list after the write, as
// the vote's is, and a vote the host does not hold is a successful no-op. The
// mock host removes it from the registry as the real one removes it from the
// profile, so a read that follows finds it gone.
export function useRetractModReviewVote() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: RetractModReviewVoteData) => ({
      modId: request.modId,
      votes: (mockData.reviewVotes[request.modId] ?? []).filter(
        (vote) => vote.reviewId !== request.reviewId
      ),
      succeeded: true,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    RetractModReviewVoteData,
    RetractModReviewVoteReplyData
  >('retractModReviewVote', selector);
  return {
    retractModReviewVote: result.postMessage,
    retractModReviewVotePending: result.pending,
  };
}

export function useGetInstalledMods() {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({ installedMods: mockData.installedMods }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    NoData,
    GetInstalledModsReplyData
  >('getInstalledMods', selector);
  return {
    getInstalledMods: result.postMessage,
    getInstalledModsPending: result.pending,
  };
}

export function useGetFeaturedMods() {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({ featuredMods: mockData.featuredMods }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    NoData,
    GetFeaturedModsReplyData
  >('getFeaturedMods', selector);
  return {
    getFeaturedMods: result.postMessage,
    getFeaturedModsPending: result.pending,
  };
}

export function useGetModSourceData() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: GetModSourceDataData) => ({
      modId: request.modId,
      data: mockData.installedModSourceData(request.modId),
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    GetModSourceDataData,
    GetModSourceDataReplyData
  >('getModSourceData', selector);
  return {
    getModSourceData: result.postMessage,
    getModSourceDataPending: result.pending,
  };
}

export function useGetRepositoryModSourceData() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: GetRepositoryModSourceDataData) => ({
      modId: request.modId,
      version: request.version,
      data: mockData.modVersionSource(request.modId, request.version),
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    GetRepositoryModSourceDataData,
    GetRepositoryModSourceDataReplyData
  >('getRepositoryModSourceData', selector);
  return {
    getRepositoryModSourceData: result.postMessage,
    getRepositoryModSourceDataPending: result.pending,
  };
}

export function useGetModVersions() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: GetModVersionsData) => ({
      modId: request.modId,
      versions: mockData.modVersions,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    GetModVersionsData,
    GetModVersionsReplyData
  >('getModVersions', selector);
  return {
    getModVersions: result.postMessage,
    getModVersionsPending: result.pending,
  };
}

export function useGetAppSettings() {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({ appSettings: mockData.appSettings }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    NoData,
    GetAppSettingsReplyData
  >('getAppSettings', selector);
  return {
    getAppSettings: result.postMessage,
    getAppSettingsPending: result.pending,
  };
}

export function useUpdateAppSettings() {
  // The reply echoes the settings that were asked for, which is what the caller
  // merges into its view of them.
  const selector = useCallback(
    (mockData: MockDataRegistry, request: UpdateAppSettingsData) => ({
      appSettings: request.appSettings,
      succeeded: true,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    UpdateAppSettingsData,
    UpdateAppSettingsReplyData
  >('updateAppSettings', selector);
  return {
    updateAppSettings: result.postMessage,
    updateAppSettingsPending: result.pending,
  };
}

export function useGetModSettings() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: GetModSettingsData) => ({
      modId: request.modId,
      settings:
        mockData.savedModSettings[request.modId] ??
        mockData.modSettings(request.modId),
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    GetModSettingsData,
    GetModSettingsReplyData
  >('getModSettings', selector);
  return {
    getModSettings: result.postMessage,
    getModSettingsPending: result.pending,
  };
}

export function useSetModSettings() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: SetModSettingsData) => ({
      modId: request.modId,
      succeeded: true,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    SetModSettingsData,
    SetModSettingsReplyData
  >('setModSettings', selector);
  return {
    setModSettings: result.postMessage,
    setModSettingsPending: result.pending,
  };
}

// The options a mod wrote at runtime for its `$dynamicSelect` settings, keyed by
// setting path. Asked for when the settings editor mounts and again as a dynamic
// dropdown opens; nothing pushes a change from the engine.
export function useGetModDynamicSelectOptions() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: GetModDynamicSelectOptionsData) => ({
      modId: request.modId,
      options: mockData.modDynamicSelectOptions,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    GetModDynamicSelectOptionsData,
    GetModDynamicSelectOptionsReplyData
  >('getModDynamicSelectOptions', selector);
  return {
    getModDynamicSelectOptions: result.postMessage,
    getModDynamicSelectOptionsPending: result.pending,
  };
}

// The host's native Open dialog, for a `filePath` setting, or its folder dialog
// for a `folderPath` one: a file input inside the webview yields no filesystem
// path, so the host owns the pick. The reply carries the path, or says the
// dialog was dismissed.
export function usePickFilePath() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: PickFilePathData) => ({
      path: request.folder ? mockData.pickedFolderPath : mockData.pickedFilePath,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    PickFilePathData,
    PickFilePathReplyData
  >('pickFilePath', selector);
  return {
    pickFilePath: result.postMessage,
    pickFilePathPending: result.pending,
  };
}

// The font families installed on the host machine, for a `fontFamily` setting's
// completion; the webview cannot enumerate them on its own. Asked once per
// settings editor that declares such a setting.
export function useListFontFamilies() {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({ families: mockData.fontFamilies }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    ListFontFamiliesData,
    ListFontFamiliesReplyData
  >('listFontFamilies', selector);
  return {
    listFontFamilies: result.postMessage,
    listFontFamiliesPending: result.pending,
  };
}

// The next keyboard shortcut pressed, recorded by the host for a `hotkey`
// setting's badge. The reply lands when the capture ends - with the shortcut
// in the stored form, or null with the reason there is none - and the held
// modifiers arrive as hotkeyCaptureProgress events on the way. A host that
// cannot record answers null with the error attached, which the editor takes
// as the cue for its manual editor.
export function useCaptureHotkey() {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({ hotkey: mockData.hotkeyCapture.hotkey }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    CaptureHotkeyData,
    CaptureHotkeyReplyData
  >('captureHotkey', selector);
  return {
    captureHotkey: result.postMessage,
    captureHotkeyPending: result.pending,
  };
}

// End the capture in flight, if any; its own reply still arrives, as canceled.
export function useCancelCaptureHotkey() {
  // A mock capture is answered within a task, so a cancel that gets here names
  // one that has already ended.
  const selector = useCallback(() => ({ signaled: false }), []);
  const result = usePostMessageWithReplyWithMock<
    CancelCaptureHotkeyData,
    CancelCaptureHotkeyReplyData
  >('cancelCaptureHotkey', selector);
  return {
    cancelCaptureHotkey: result.postMessage,
    cancelCaptureHotkeyPending: result.pending,
  };
}

export function useGetModConfig() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: GetModConfigData) => ({
      modId: request.modId,
      config: mockData.modConfig[request.modId] || null,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    GetModConfigData,
    GetModConfigReplyData
  >('getModConfig', selector);
  return {
    getModConfig: result.postMessage,
    getModConfigPending: result.pending,
  };
}

export function useUpdateModConfig() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: UpdateModConfigData) => {
      // The host refuses a suppression outside the grammar, so the mock refuses
      // it too: a write no host would take must not read here as one that was.
      const suppression = request.config.updatesDisabledForVersion;
      if (suppression !== undefined && !isValidSuppression(suppression)) {
        return {
          modId: request.modId,
          succeeded: false,
          error: {
            code: 'INVALID_REQUEST',
            message: `Not a valid update suppression: "${suppression}"`,
          },
        };
      }
      return {
        modId: request.modId,
        succeeded: true,
      };
    },
    []
  );
  const result = usePostMessageWithReplyWithMock<
    UpdateModConfigData,
    UpdateModConfigReplyData
  >('updateModConfig', selector);
  return {
    updateModConfig: result.postMessage,
    updateModConfigPending: result.pending,
  };
}

// Switches the user's own configuration of a mod on or off. The reply carries the
// mod's whole config in the new scope, which the host also pushes as
// setNewModConfig.
export function useSetModUserScope() {
  // The mock copies nothing: the user's configuration starts out equal to the
  // global one, which is all a copy would show.
  const selector = useCallback(
    (mockData: MockDataRegistry, request: SetModUserScopeData) =>
      mockUserScopeReply(mockData, request.modId, (current) =>
        request.active ? 'active' : current === 'none' ? 'none' : 'inactive'
      ),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    SetModUserScopeData,
    SetModUserScopeReplyData
  >('setModUserScope', selector);
  return {
    setModUserScope: result.postMessage,
    setModUserScopePending: result.pending,
  };
}

// Removes the user's own configuration of a mod; the reply and the echo carry
// the global config it leaves in force.
export function useDeleteModUserConfig() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: DeleteModUserConfigData) =>
      mockUserScopeReply(mockData, request.modId, () => 'none'),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    DeleteModUserConfigData,
    DeleteModUserConfigReplyData
  >('deleteModUserConfig', selector);
  return {
    deleteModUserConfig: result.postMessage,
    deleteModUserConfigPending: result.pending,
  };
}

export function useGetRepositoryMods() {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({ mods: repositoryModsListing(mockData) }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    NoData,
    GetRepositoryModsReplyData
  >('getRepositoryMods', selector);
  return {
    getRepositoryMods: result.postMessage,
    getRepositoryModsPending: result.pending,
  };
}

export function useStartUpdate() {
  // The reply is not the end of an update: the host follows it with download and
  // install events, and finally replaces the running app. A mock host does none
  // of that, so reporting the installer as started would leave the modal waiting
  // on events that never arrive, with nothing to close it by. Reporting that no
  // installer could be started is true of this host and terminal on screen.
  const selector = useCallback(
    () => ({
      succeeded: false,
      error: 'There is no Windhawk update to install against mock data.',
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    NoData,
    StartUpdateReplyData
  >('startUpdate', selector);
  return {
    startUpdate: result.postMessage,
    startUpdatePending: result.pending,
  };
}

export function useCancelUpdate() {
  const selector = useCallback(() => ({ succeeded: true }), []);
  const result = usePostMessageWithReplyWithMock<
    NoData,
    CancelUpdateReplyData
  >('cancelUpdate', selector);
  return {
    cancelUpdate: result.postMessage,
    cancelUpdatePending: result.pending,
  };
}

export function useStartInstallDevTools() {
  // The dev-tools twin of useStartUpdate's selector, for the same reason: this
  // host runs no installer and pushes none of the progress the modal waits on.
  const selector = useCallback(
    () => ({
      succeeded: false,
      error: 'The development tools cannot be installed against mock data.',
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    NoData,
    StartInstallDevToolsReplyData
  >('startInstallDevTools', selector);
  return {
    startInstallDevTools: result.postMessage,
    startInstallDevToolsPending: result.pending,
  };
}

export function useCancelInstallDevTools() {
  const selector = useCallback(() => ({ succeeded: true }), []);
  const result = usePostMessageWithReplyWithMock<
    NoData,
    CancelInstallDevToolsReplyData
  >('cancelInstallDevTools', selector);
  return {
    cancelInstallDevTools: result.postMessage,
    cancelInstallDevToolsPending: result.pending,
  };
}

export function useEnableEditedMod() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: EnableEditedModData) => ({
      enabled: request.enable,
      succeeded: true,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    EnableEditedModData,
    EnableEditedModReplyData
  >('enableEditedMod', selector);
  return {
    enableEditedMod: result.postMessage,
    enableEditedModPending: result.pending,
  };
}

export function useEnableEditedModLogging() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: EnableEditedModLoggingData) => ({
      enabled: request.enable,
      succeeded: true,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    EnableEditedModLoggingData,
    EnableEditedModLoggingReplyData
  >('enableEditedModLogging', selector);
  return {
    enableEditedModLogging: result.postMessage,
    enableEditedModLoggingPending: result.pending,
  };
}

export function useCompileEditedMod() {
  // A build that succeeded compiled the source as it stands, so what the editor
  // holds is no longer ahead of what is compiled - which is what clearModified
  // tells the sidebar to stop marking.
  const selector = useCallback(
    () => ({ succeeded: true, clearModified: true }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    CompileEditedModData,
    CompileEditedModReplyData
  >('compileEditedMod', selector);
  return {
    compileEditedMod: result.postMessage,
    compileEditedModPending: result.pending,
  };
}

export function useDeleteEditedMod() {
  const selector = useCallback(() => ({ succeeded: true }), []);
  const result = usePostMessageWithReplyWithMock<
    NoData,
    DeleteEditedModReplyData
  >('deleteEditedMod', selector);
  return {
    deleteEditedMod: result.postMessage,
    deleteEditedModPending: result.pending,
  };
}

export function useExitEditorMode() {
  const selector = useCallback(() => ({ succeeded: true }), []);
  const result = usePostMessageWithReplyWithMock<
    ExitEditorModeData,
    ExitEditorModeReplyData
  >('exitEditorMode', selector);
  return {
    exitEditorMode: result.postMessage,
    exitEditorModePending: result.pending,
  };
}

// User-data export: hand the selection to the host, which calls the core and then
// runs the native Save dialog around the returned archive. The host owns the file
// I/O, so the reply only reports success/cancel and any per-mod export warnings.
export function useExportUserData() {
  const selector = useCallback(
    () => ({ succeeded: true, summary: { warnings: [] } }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    ExportUserDataData,
    ExportUserDataReplyData
  >('exportUserData', selector);
  return {
    exportUserData: result.postMessage,
    exportUserDataPending: result.pending,
  };
}

// User-data inspect: the host validates an archive and projects its manifest. Sent
// with an `archive` the user pasted, or without one to let the host pick and read a
// file. The reply echoes the archive bytes so a follow-up import needs no read.
export function useInspectUserData() {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: InspectUserDataData) => ({
      succeeded: true,
      // The mock manifest stands in for any archive, but a pasted one is echoed
      // back as sent, the way a host's inspect echoes what it validated.
      manifest: mockData.userDataManifest,
      archive: request.archive ?? mockData.userDataArchive,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    InspectUserDataData,
    InspectUserDataReplyData
  >('inspectUserData', selector);
  return {
    inspectUserData: result.postMessage,
    inspectUserDataPending: result.pending,
  };
}

// User-data import: an async operation (it compiles). The reply here is the terminal
// result; per-mod progress arrives separately as importUserDataProgress events (see
// useImportUserDataProgress), mirroring startUpdate + updateDownloadProgress.
export function useImportUserData() {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({
      succeeded: true,
      summary: mockData.userDataImportSummary,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    ImportUserDataData,
    ImportUserDataReplyData
  >('importUserData', selector);
  return {
    importUserData: result.postMessage,
    importUserDataPending: result.pending,
  };
}

// Cancel an in-flight import (mirrors cancelUpdate); the import's own terminal reply
// still arrives with the summary of what completed before the cancel.
export function useCancelImportUserData() {
  const selector = useCallback(() => ({ succeeded: true }), []);
  const result = usePostMessageWithReplyWithMock<
    NoData,
    CancelImportUserDataReplyData
  >('cancelImportUserData', selector);
  return {
    cancelImportUserData: result.postMessage,
    cancelImportUserDataPending: result.pending,
  };
}

////////////////////////////////////////////////////////////
// Events.

/**
 * React hook that listens for event messages from the extension.
 */
function useEventMessageWithHandler<T>(
  eventName: string,
  handler: (data: T) => void
) {
  if (WEBPACK_IS_WEBSITE) {
    throw new Error(
      `useEventMessageWithHandler("${eventName}") must not be called in browser mode`
    );
  }

  useEffect(() => {
    const listener = (event: MessageEvent<MessageAny>) => {
      if (!isFromHostWindow(event)) {
        return;
      }

      const data = event.data;
      if (data.type === 'event' && data.command === eventName) {
        handler(data.data as T);
      }
    };

    window.addEventListener('message', listener);
    return () => window.removeEventListener('message', listener);
  }, [eventName, handler]);
}

/// #if HAS_MOCKS
/**
 * React hook that listens for event messages with automatic mock data injection.
 * In mock mode, automatically calls the handler with mock data on mount.
 */
function useEventMessageWithMockDev<T>(
  eventName: string,
  handler: (data: T) => void,
  mockDataSelector?: (mockData: MockDataRegistry) => T
) {
  const { isMockMode, mockData } = useMockContext();

  // Always call the real event handler to maintain hook order
  useEventMessageWithHandler<T>(eventName, handler);

  // In mock mode, automatically trigger the handler with mock data
  useEffect(() => {
    if (isMockMode && mockDataSelector) {
      // Simulate async event delivery
      setTimeout(() => {
        const mockEvent = mockDataSelector(mockData);
        handler(mockEvent);
      }, 0);
    }
  }, [isMockMode, mockData, mockDataSelector, handler]);
}
/// #else
/**
 * Production version: simply forwards to useEventMessageWithHandler.
 * Mock data selector is ignored.
 */
function useEventMessageWithMockProd<T>(
  eventName: string,
  handler: (data: T) => void,
  _mockDataSelector?: unknown
) {
  useEventMessageWithHandler<T>(eventName, handler);
}
/// #endif

const useEventMessageWithMock = WEBPACK_HAS_MOCKS
  ? useEventMessageWithMockDev
  : useEventMessageWithMockProd;

export function useSetNewAppSettings(
  handler: (data: SetNewAppSettingsData) => void
) {
  useEventMessageWithHandler<SetNewAppSettingsData>(
    'setNewAppSettings',
    handler
  );
}

export function useUpdateDownloadProgress(
  handler: (data: UpdateDownloadProgressEventData) => void
) {
  useEventMessageWithHandler<UpdateDownloadProgressEventData>(
    'updateDownloadProgress',
    handler
  );
}

export function useUpdateInstalling(
  handler: (data: UpdateInstallingEventData) => void
) {
  useEventMessageWithHandler<UpdateInstallingEventData>(
    'updateInstalling',
    handler
  );
}

export function useDevToolsInstallDownloadProgress(
  handler: (data: DevToolsInstallDownloadProgressEventData) => void
) {
  useEventMessageWithHandler<DevToolsInstallDownloadProgressEventData>(
    'devToolsInstallDownloadProgress',
    handler
  );
}

export function useDevToolsInstalling(
  handler: (data: DevToolsInstallingEventData) => void
) {
  useEventMessageWithHandler<DevToolsInstallingEventData>(
    'devToolsInstalling',
    handler
  );
}

export function useUpdateInstalledModsDetails(
  handler: (data: UpdateInstalledModsDetailsData) => void
) {
  useEventMessageWithHandler<UpdateInstalledModsDetailsData>(
    'updateInstalledModsDetails',
    handler
  );
}

export function useReloadInstalledMods(handler: (data: NoData) => void) {
  useEventMessageWithHandler<NoData>('reloadInstalledMods', handler);
}

export function useSetNewModConfig(
  handler: (data: SetNewModConfigData) => void
) {
  useEventMessageWithHandler<SetNewModConfigData>(
    'setNewModConfig',
    handler
  );
}

export function useSetEditedModId(handler: (data: SetEditedModIdData) => void) {
  useEventMessageWithHandler<SetEditedModIdData>('setEditedModId', handler);
}

export function useCompileEditedModStart(handler: (data: NoData) => void) {
  useEventMessageWithHandler<NoData>('compileEditedModStart', handler);
}

export function useEditedModWasModified(handler: (data: NoData) => void) {
  useEventMessageWithHandler<NoData>('editedModWasModified', handler);
}

export function useSetEditedModDetails(
  handler: (data: SetEditedModDetailsData) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry) => mockData.sidebarModDetails,
    []
  );
  useEventMessageWithMock<SetEditedModDetailsData>(
    'setEditedModDetails',
    handler,
    selector
  );
}

// The modifiers a hotkey capture in flight reports held, on every change of the
// set. Plain (no mock injection at mount): the mock host pushes these ahead of
// its captureHotkey reply, on the window this listens on.
export function useHotkeyCaptureProgress(
  handler: (data: HotkeyCaptureProgressEventData) => void
) {
  useEventMessageWithHandler<HotkeyCaptureProgressEventData>(
    'hotkeyCaptureProgress',
    handler
  );
}

// Per-mod progress an in-flight import emits. Plain (no mock injection): a mock host
// has no live import to stream, so its terminal reply carries the whole summary and
// this stays quiet in mock mode.
export function useImportUserDataProgress(
  handler: (data: ImportUserDataProgressEventData) => void
) {
  useEventMessageWithHandler<ImportUserDataProgressEventData>(
    'importUserDataProgress',
    handler
  );
}
