import { useCallback, useEffect, useRef, useState } from 'react';

import backendApi from './backendApi';
import { promptDevToolsInstall } from './devToolsInstall';
import { isWireError, surfaceWireError } from './feedback';
import {
  WEBVIEW_IPC_CONTRACT_VERSION,
  type CancelInstallDevToolsReplyData,
  type CancelUpdateReplyData,
  type CompileEditedModData,
  type CompileEditedModReplyData,
  type CancelImportUserDataReplyData,
  type CompileModData,
  type CompileModReplyData,
  type DeleteModData,
  type DeleteModReplyData,
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
  type GetModConfigData,
  type GetModConfigReplyData,
  type GetModSettingsData,
  type GetModSettingsReplyData,
  type GetModSourceDataData,
  type GetModSourceDataReplyData,
  type GetModVersionsData,
  type GetModVersionsReplyData,
  type GetRepositoryModSourceDataData,
  type GetRepositoryModSourceDataReplyData,
  type GetRepositoryModsReplyData,
  type ImportUserDataData,
  type ImportUserDataProgressEventData,
  type ImportUserDataReplyData,
  type InspectUserDataData,
  type InspectUserDataReplyData,
  type InstallModData,
  type InstallModReplyData,
  type NoData,
  type SetEditedModDetailsData,
  type SetEditedModIdData,
  type SetModSettingsData,
  type SetModSettingsReplyData,
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
  type UpdateModRatingReplyData
} from './webviewIPCMessages';
/// #if HAS_MOCKS
import type { MockDataRegistry } from './mocking';
import { useMockContext } from './mocking';
import { applyScenarioReply } from './mocking/mockScenarios';
/// #endif

// Use webpack constants for conditional compilation
declare const WEBPACK_IS_WEBSITE: boolean;
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

////////////////////////////////////////////////////////////
// Messages.

// The launch entry points (createNewMod / editMod / forkMod) are `messageWithReply`s
// so the native UI can react. They are called as plain actions from many places, so
// each handles its own reply here rather than through a component hook: a standard
// error object is auto-surfaced (as the reply hook does for other commands), and a
// `uiMissing` reply opens the "install development tools" modal through the registered
// prompt seam. Success is a no-op.
// Resolves true when the dev action is opening the editor, and false when it is
// not - a wire error was surfaced, or dev tools are missing so the install modal
// was raised instead. Callers use this to drop any pending-launch UI when the
// editor will not appear.
function dispatchDevActionReply(
  promise: Promise<DevActionReplyData>
): Promise<boolean> {
  return promise
    .then((reply) => {
      if (isWireError(reply.error)) {
        surfaceWireError(reply.error);
        return false;
      } else if (reply.uiMissing) {
        promptDevToolsInstall();
        return false;
      }
      return true;
    })
    .catch((error) => {
      // Cancellation is expected (e.g. a superseding request); anything else rethrows.
      if (error instanceof MessageCancelledError) {
        return false;
      }
      throw error;
    });
}

export function createNewMod() {
  return dispatchDevActionReply(
    sendMessageWithReply<NoData, DevActionReplyData>('createNewMod', {}).promise
  );
}

export function editMod(data: EditModData) {
  return dispatchDevActionReply(
    sendMessageWithReply<EditModData, DevActionReplyData>('editMod', data).promise
  );
}

export function forkMod(data: ForkModData) {
  return dispatchDevActionReply(
    sendMessageWithReply<ForkModData, DevActionReplyData>('forkMod', data).promise
  );
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
 * Hand a reply to its handler, surfacing a command failure on the way.
 *
 * Every reply funnels through here, so a handler that swallows the error into its
 * command-specific default (null / empty / succeeded:false) still gets the failure
 * shown once. The handler still runs: it owns the data-shape side of the failure.
 * Only the standard error OBJECT triggers this; startUpdate's error STRING is left
 * to its own modal.
 */
function dispatchReply<TReply, TContext>(
  reply: TReply,
  ctx: TContext | undefined,
  handler: ((data: TReply, context?: TContext) => void) | undefined
) {
  const replyError = (reply as { error?: unknown }).error;
  if (isWireError(replyError)) {
    surfaceWireError(replyError);
  }
  handler?.(reply, ctx);
}

/**
 * What a request is about, for deciding which other requests it supersedes.
 *
 * Two requests with the same key are about the same thing, so the later one is the
 * answer and a reply for the earlier one that arrives after it is stale. Requests
 * with different keys are independent and both replies matter. Omitting the option
 * puts every request under one key, which is right for a screen-wide read
 * (`getInstalledMods`) where each call replaces the last.
 */
type SupersedeKey<TPostMessage> = (data: TPostMessage) => string;

// The key every request shares when a hook names none.
const SINGLE_STREAM_KEY = '';

/**
 * The supersede key for anything addressed to one mod: a request about mod A says
 * nothing about mod B, so the two never displace each other's reply, while two
 * requests about the same mod still resolve to the later one. Every per-mod
 * command and read uses it, because one hook instance serves a whole screen's
 * worth of mods.
 */
const byModId = <TPostMessage extends { modId: string }>(data: TPostMessage) =>
  data.modId;

/**
 * React hook wrapper for sendMessageWithReply that manages pending state and context.
 *
 * One hook instance serves every target a screen acts on - a mods browser mounts a
 * single useEnableMod for its whole grid - so several requests can be in flight at
 * once. They all run to completion and `pending` covers the whole set. A reply is
 * handed to the handler unless a newer request ABOUT THE SAME THING has already
 * been answered, which is what `supersedesBy` names: without it, a slow disable of
 * one mod would be discarded by a fast disable of another, and the first mod's
 * switch would sit on a state the host had already left. Both hosts dispatch each
 * envelope concurrently (windhawk-core's wh_ipc spawns a worker per envelope; the
 * VSCode extension does not await its handler), so reply order follows how long
 * the work took, not the order the requests were sent.
 */
function usePostMessageWithReplyWithHandler<
  TPostMessage extends Record<string, unknown>,
  TReply,
  TContext extends Record<string, unknown>
>(
  eventName: string,
  handler: (data: TReply, context?: TContext) => void,
  supersedesBy?: SupersedeKey<TPostMessage>
) {
  if (WEBPACK_IS_WEBSITE) {
    throw new Error(
      `usePostMessageWithReplyWithHandler("${eventName}") must not be called in browser mode`
    );
  }

  const [pending, setPending] = useState(false);
  const [context, setContext] = useState<TContext>();
  // The cancel function of every request still awaiting a reply, keyed by request
  // id, so unmount can abandon all of them and `pending` can track the set.
  const inFlightRef = useRef(new Map<number, () => void>());
  const requestIdRef = useRef(0);
  // Per supersede key, the newest request whose reply was handed to the handler. A
  // reply older than that one arrived out of order and is stale.
  const deliveredIdRef = useRef(new Map<string, number>());
  const handlerRef = useRef<typeof handler>();
  // Held in a ref, not a dependency: postMessage's identity has to stay stable
  // across renders, because the call sites drive it from an effect keyed on it.
  const supersedesByRef = useRef(supersedesBy);

  // Keep handler ref up to date
  useEffect(() => {
    handlerRef.current = handler;
  }, [handler]);

  useEffect(() => {
    supersedesByRef.current = supersedesBy;
  }, [supersedesBy]);

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
    async (data: TPostMessage, ctx?: TContext) => {
      // Generate a unique ID for this request to detect stale responses
      const currentRequestId = ++requestIdRef.current;
      const supersedeKeyOf = supersedesByRef.current;
      const supersedeKey = supersedeKeyOf
        ? supersedeKeyOf(data)
        : SINGLE_STREAM_KEY;

      setPending(true);
      setContext(ctx);

      const { promise, cancel } = sendMessageWithReply<TPostMessage, TReply>(
        eventName,
        data
      );
      const inFlight = inFlightRef.current;
      inFlight.set(currentRequestId, cancel);

      try {
        const reply = await promise;

        const delivered = deliveredIdRef.current;
        if (currentRequestId > (delivered.get(supersedeKey) ?? 0)) {
          delivered.set(supersedeKey, currentRequestId);
          dispatchReply(reply, ctx, handlerRef.current);
        }
      } catch (error) {
        // Don't throw MessageCancelledError - cancellation is expected behavior
        if (!(error instanceof MessageCancelledError)) {
          throw error;
        }
      } finally {
        inFlight.delete(currentRequestId);
        if (inFlight.size === 0) {
          setPending(false);
          setContext(undefined);
        }
      }
    },
    [eventName]
  );

  return { postMessage, pending, context };
}

/// #if HAS_MOCKS
/**
 * Wrapper for usePostMessageWithReplyWithHandler that adds automatic mock data support.
 * When running in development mode (without VSCode API), automatically returns mock data
 * instead of making IPC calls.
 *
 * @param eventName - The IPC event name
 * @param handler - Reply handler function
 * @param mockDataSelector - Function to extract mock data from MockDataRegistry and request (only used in mock mode)
 * @param supersedesBy - What a request is about, for the real path's stale-reply guard
 * @returns Same interface as usePostMessageWithReplyWithHandler
 */
function usePostMessageWithReplyWithMockDev<
  TPostMessage extends Record<string, unknown>,
  TReply,
  TContext extends Record<string, unknown>
>(
  eventName: string,
  handler: (data: TReply, context?: TContext) => void,
  mockDataSelector?: (mockData: MockDataRegistry, request: TPostMessage) => TReply,
  supersedesBy?: SupersedeKey<TPostMessage>
) {
  const { isMockMode, mockData } = useMockContext();

  // Always call the real hook to maintain hook order (even if we won't use it)
  const realResult = usePostMessageWithReplyWithHandler<
    TPostMessage,
    TReply,
    TContext
  >(eventName, handler, supersedesBy);

  // The mock reply is delivered a tick later, by which time the handler may have
  // been re-created over newer state - which is what a host's reply would meet.
  // Keep the latest one, as the real hook does, so a handler that tests against
  // state its own request set does not miss its reply.
  const handlerRef = useRef(handler);
  useEffect(() => {
    handlerRef.current = handler;
  }, [handler]);

  // Mock mode: create a simulated IPC call (always create it to maintain hook order)
  const mockPostMessage = useCallback(
    (data: TPostMessage, ctx?: TContext) => {
      // There is no host to hand the envelope to, so echo it on the window the way
      // the real transport hands it over. A page watching the app - the browser
      // preview's devtools, an E2E spec - can then see the exact request an action
      // produces. The app itself ignores it: its listeners take only 'reply' and
      // 'event' envelopes.
      window.postMessage(
        { type: 'messageWithReply', command: eventName, data },
        '*'
      );
      // Simulate async behavior
      if (mockDataSelector) {
        setTimeout(() => {
          const mockReply = applyScenarioReply(
            eventName,
            mockDataSelector(mockData, data)
          );
          // Through the same dispatch the host round trip uses, so a scenario's
          // failure reply surfaces its error exactly as a real one would.
          dispatchReply(mockReply, ctx, handlerRef.current);
        }, 0);
      }
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
    context: undefined,
  };
}
/// #else
/**
 * Production version: simply forwards to usePostMessageWithReplyWithHandler.
 * Mock data selector is ignored.
 */
function usePostMessageWithReplyWithMockProd<
  TPostMessage extends Record<string, unknown>,
  TReply,
  TContext extends Record<string, unknown>
>(
  eventName: string,
  handler: (data: TReply, context?: TContext) => void,
  _mockDataSelector?: unknown,
  supersedesBy?: SupersedeKey<TPostMessage>
) {
  return usePostMessageWithReplyWithHandler<TPostMessage, TReply, TContext>(
    eventName,
    handler,
    supersedesBy
  );
}
/// #endif

const usePostMessageWithReplyWithMock = WEBPACK_HAS_MOCKS
  ? usePostMessageWithReplyWithMockDev
  : usePostMessageWithReplyWithMockProd;

export function useGetInitialAppSettings<
  TContext extends Record<string, unknown>
>(handler: (data: GetInitialAppSettingsReplyData, context?: TContext) => void) {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({
      // The mock host is this same build, so it reports our own contract version.
      contractVersion: WEBVIEW_IPC_CONTRACT_VERSION,
      appUISettings: mockData.appUISettings,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    NoData,
    GetInitialAppSettingsReplyData,
    TContext
  >('getInitialAppSettings', handler, selector);
  return {
    getInitialAppSettings: result.postMessage,
    getInitialAppSettingsPending: result.pending,
    getInitialAppSettingsContext: result.context,
  };
}

export function useInstallMod<TContext extends Record<string, unknown>>(
  handler: (data: InstallModReplyData, context?: TContext) => void
) {
  // A mock install always succeeds, reporting the mod as the repository describes
  // it and compiled with the config a fresh install carries.
  const selector = useCallback(
    (mockData: MockDataRegistry, request: InstallModData) => ({
      modId: request.modId,
      installedModDetails: {
        metadata: mockData.modVersionSource(request.modId).metadata,
        config: {
          ...mockData.newModConfig,
          disabled: !!request.disabled,
          loggingEnabled: !!request.loggingEnabled,
        },
      },
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    InstallModData,
    InstallModReplyData,
    TContext
  >(
    'installMod',
    // A local compile (alwaysCompileModsLocally) with the development tools absent
    // replies uiMissing and starts no install; raise the install-dev-tools modal, as
    // the launch entry points do, and skip the details handler (nothing to apply).
    // Centralized here so every install call site inherits it.
    useCallback(
      (data: InstallModReplyData, context?: TContext) => {
        if (data.uiMissing) {
          promptDevToolsInstall();
          return;
        }
        handler(data, context);
      },
      [handler]
    ),
    selector,
    byModId
  );
  return {
    installMod: result.postMessage,
    installModPending: result.pending,
    installModContext: result.context,
  };
}

export function useCompileMod<TContext extends Record<string, unknown>>(
  handler: (data: CompileModReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: CompileModData) => ({
      modId: request.modId,
      compiledModDetails: {
        metadata: mockData.installedModSourceData.metadata,
        config: { ...mockData.newModConfig },
      },
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    CompileModData,
    CompileModReplyData,
    TContext
  >(
    'compileMod',
    // A recompile always compiles locally; with the development tools absent it replies
    // uiMissing and starts no compile. Handled exactly like installMod's uiMissing.
    useCallback(
      (data: CompileModReplyData, context?: TContext) => {
        if (data.uiMissing) {
          promptDevToolsInstall();
          return;
        }
        handler(data, context);
      },
      [handler]
    ),
    selector,
    byModId
  );
  return {
    compileMod: result.postMessage,
    compileModPending: result.pending,
    compileModContext: result.context,
  };
}

export function useEnableMod<TContext extends Record<string, unknown>>(
  handler: (data: EnableModReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: EnableModData) => ({
      modId: request.modId,
      enabled: request.enable,
      succeeded: true,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    EnableModData,
    EnableModReplyData,
    TContext
  >('enableMod', handler, selector, byModId);
  return {
    enableMod: result.postMessage,
    enableModPending: result.pending,
    enableModContext: result.context,
  };
}

export function useDeleteMod<TContext extends Record<string, unknown>>(
  handler: (data: DeleteModReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: DeleteModData) => ({
      modId: request.modId,
      succeeded: true,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    DeleteModData,
    DeleteModReplyData,
    TContext
  >('deleteMod', handler, selector, byModId);
  return {
    deleteMod: result.postMessage,
    deleteModPending: result.pending,
    deleteModContext: result.context,
  };
}

export function useUpdateModRating<TContext extends Record<string, unknown>>(
  handler: (data: UpdateModRatingReplyData, context?: TContext) => void
) {
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
    UpdateModRatingReplyData,
    TContext
  >('updateModRating', handler, selector, byModId);
  return {
    updateModRating: result.postMessage,
    updateModRatingPending: result.pending,
    updateModRatingContext: result.context,
  };
}

export function useGetInstalledMods<TContext extends Record<string, unknown>>(
  handler: (data: GetInstalledModsReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({ installedMods: mockData.installedMods }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    NoData,
    GetInstalledModsReplyData,
    TContext
  >('getInstalledMods', handler, selector);
  return {
    getInstalledMods: result.postMessage,
    getInstalledModsPending: result.pending,
    getInstalledModsContext: result.context,
  };
}

export function useGetFeaturedMods<TContext extends Record<string, unknown>>(
  handler: (data: GetFeaturedModsReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({ featuredMods: mockData.featuredMods }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    NoData,
    GetFeaturedModsReplyData,
    TContext
  >('getFeaturedMods', handler, selector);
  return {
    getFeaturedMods: result.postMessage,
    getFeaturedModsPending: result.pending,
    getFeaturedModsContext: result.context,
  };
}

export function useGetModSourceData<TContext extends Record<string, unknown>>(
  handler: (data: GetModSourceDataReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: GetModSourceDataData) => ({
      modId: request.modId,
      data: mockData.installedModSourceData,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    GetModSourceDataData,
    GetModSourceDataReplyData,
    TContext
  >('getModSourceData', handler, selector, byModId);
  return {
    getModSourceData: result.postMessage,
    getModSourceDataPending: result.pending,
    getModSourceDataContext: result.context,
  };
}

export function useGetRepositoryModSourceData<
  TContext extends Record<string, unknown>
>(
  handler: (
    data: GetRepositoryModSourceDataReplyData,
    context?: TContext
  ) => void
) {
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
    GetRepositoryModSourceDataReplyData,
    TContext
  >('getRepositoryModSourceData', handler, selector, byModId);
  return {
    getRepositoryModSourceData: result.postMessage,
    getRepositoryModSourceDataPending: result.pending,
    getRepositoryModSourceDataContext: result.context,
  };
}

export function useGetModVersions<TContext extends Record<string, unknown>>(
  handler: (data: GetModVersionsReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: GetModVersionsData) => ({
      modId: request.modId,
      versions: mockData.modVersions,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    GetModVersionsData,
    GetModVersionsReplyData,
    TContext
  >('getModVersions', handler, selector, byModId);
  return {
    getModVersions: result.postMessage,
    getModVersionsPending: result.pending,
    getModVersionsContext: result.context,
  };
}

export function useGetAppSettings<TContext extends Record<string, unknown>>(
  handler: (data: GetAppSettingsReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({ appSettings: mockData.appSettings }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    NoData,
    GetAppSettingsReplyData,
    TContext
  >('getAppSettings', handler, selector);
  return {
    getAppSettings: result.postMessage,
    getAppSettingsPending: result.pending,
    getAppSettingsContext: result.context,
  };
}

export function useUpdateAppSettings<TContext extends Record<string, unknown>>(
  handler: (data: UpdateAppSettingsReplyData, context?: TContext) => void
) {
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
    UpdateAppSettingsReplyData,
    TContext
  >('updateAppSettings', handler, selector);
  return {
    updateAppSettings: result.postMessage,
    updateAppSettingsPending: result.pending,
    updateAppSettingsContext: result.context,
  };
}

export function useGetModSettings<TContext extends Record<string, unknown>>(
  handler: (data: GetModSettingsReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: GetModSettingsData) => ({
      modId: request.modId,
      settings: mockData.modSettings as Record<string, string | number>,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    GetModSettingsData,
    GetModSettingsReplyData,
    TContext
  >('getModSettings', handler, selector, byModId);
  return {
    getModSettings: result.postMessage,
    getModSettingsPending: result.pending,
    getModSettingsContext: result.context,
  };
}

export function useSetModSettings<TContext extends Record<string, unknown>>(
  handler: (data: SetModSettingsReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: SetModSettingsData) => ({
      modId: request.modId,
      succeeded: true,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    SetModSettingsData,
    SetModSettingsReplyData,
    TContext
  >('setModSettings', handler, selector, byModId);
  return {
    setModSettings: result.postMessage,
    setModSettingsPending: result.pending,
    setModSettingsContext: result.context,
  };
}

export function useGetModConfig<TContext extends Record<string, unknown>>(
  handler: (data: GetModConfigReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: GetModConfigData) => ({
      modId: request.modId,
      config: mockData.modConfig[request.modId] || null,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    GetModConfigData,
    GetModConfigReplyData,
    TContext
  >('getModConfig', handler, selector, byModId);
  return {
    getModConfig: result.postMessage,
    getModConfigPending: result.pending,
    getModConfigContext: result.context,
  };
}

export function useUpdateModConfig<TContext extends Record<string, unknown>>(
  handler: (data: UpdateModConfigReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry, request: UpdateModConfigData) => ({
      modId: request.modId,
      succeeded: true,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    UpdateModConfigData,
    UpdateModConfigReplyData,
    TContext
  >('updateModConfig', handler, selector, byModId);
  return {
    updateModConfig: result.postMessage,
    updateModConfigPending: result.pending,
    updateModConfigContext: result.context,
  };
}

export function useGetRepositoryMods<TContext extends Record<string, unknown>>(
  handler: (data: GetRepositoryModsReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({ mods: mockData.repositoryMods }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    NoData,
    GetRepositoryModsReplyData,
    TContext
  >('getRepositoryMods', handler, selector);
  return {
    getRepositoryMods: result.postMessage,
    getRepositoryModsPending: result.pending,
    getRepositoryModsContext: result.context,
  };
}

export function useStartUpdate<TContext extends Record<string, unknown>>(
  handler: (data: StartUpdateReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    NoData,
    StartUpdateReplyData,
    TContext
  >('startUpdate', handler);
  return {
    startUpdate: result.postMessage,
    startUpdatePending: result.pending,
    startUpdateContext: result.context,
  };
}

export function useCancelUpdate<TContext extends Record<string, unknown>>(
  handler: (data: CancelUpdateReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    NoData,
    CancelUpdateReplyData,
    TContext
  >('cancelUpdate', handler);
  return {
    cancelUpdate: result.postMessage,
    cancelUpdatePending: result.pending,
    cancelUpdateContext: result.context,
  };
}

export function useStartInstallDevTools<TContext extends Record<string, unknown>>(
  handler: (data: StartInstallDevToolsReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    NoData,
    StartInstallDevToolsReplyData,
    TContext
  >('startInstallDevTools', handler);
  return {
    startInstallDevTools: result.postMessage,
    startInstallDevToolsPending: result.pending,
    startInstallDevToolsContext: result.context,
  };
}

export function useCancelInstallDevTools<
  TContext extends Record<string, unknown>
>(handler: (data: CancelInstallDevToolsReplyData, context?: TContext) => void) {
  const result = usePostMessageWithReplyWithHandler<
    NoData,
    CancelInstallDevToolsReplyData,
    TContext
  >('cancelInstallDevTools', handler);
  return {
    cancelInstallDevTools: result.postMessage,
    cancelInstallDevToolsPending: result.pending,
    cancelInstallDevToolsContext: result.context,
  };
}

export function useEnableEditedMod<TContext extends Record<string, unknown>>(
  handler: (data: EnableEditedModReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    EnableEditedModData,
    EnableEditedModReplyData,
    TContext
  >('enableEditedMod', handler);
  return {
    enableEditedMod: result.postMessage,
    enableEditedModPending: result.pending,
    enableEditedModContext: result.context,
  };
}

export function useEnableEditedModLogging<
  TContext extends Record<string, unknown>
>(
  handler: (data: EnableEditedModLoggingReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    EnableEditedModLoggingData,
    EnableEditedModLoggingReplyData,
    TContext
  >('enableEditedModLogging', handler);
  return {
    enableEditedModLogging: result.postMessage,
    enableEditedModLoggingPending: result.pending,
    enableEditedModLoggingContext: result.context,
  };
}

export function useCompileEditedMod<TContext extends Record<string, unknown>>(
  handler: (data: CompileEditedModReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    CompileEditedModData,
    CompileEditedModReplyData,
    TContext
  >('compileEditedMod', handler);
  return {
    compileEditedMod: result.postMessage,
    compileEditedModPending: result.pending,
    compileEditedModContext: result.context,
  };
}

export function useExitEditorMode<TContext extends Record<string, unknown>>(
  handler: (data: ExitEditorModeReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    ExitEditorModeData,
    ExitEditorModeReplyData,
    TContext
  >('exitEditorMode', handler);
  return {
    exitEditorMode: result.postMessage,
    exitEditorModePending: result.pending,
    exitEditorModeContext: result.context,
  };
}

// User-data export: hand the selection to the host, which calls the core and then
// runs the native Save dialog around the returned archive. The host owns the file
// I/O, so the reply only reports success/cancel and any per-mod export warnings.
export function useExportUserData<TContext extends Record<string, unknown>>(
  handler: (data: ExportUserDataReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    () => ({ succeeded: true, summary: { warnings: [] } }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    ExportUserDataData,
    ExportUserDataReplyData,
    TContext
  >('exportUserData', handler, selector);
  return {
    exportUserData: result.postMessage,
    exportUserDataPending: result.pending,
    exportUserDataContext: result.context,
  };
}

// User-data inspect: the host validates an archive and projects its manifest. Sent
// with an `archive` the user pasted, or without one to let the host pick and read a
// file. The reply echoes the archive bytes so a follow-up import needs no read.
export function useInspectUserData<TContext extends Record<string, unknown>>(
  handler: (data: InspectUserDataReplyData, context?: TContext) => void
) {
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
    InspectUserDataReplyData,
    TContext
  >('inspectUserData', handler, selector);
  return {
    inspectUserData: result.postMessage,
    inspectUserDataPending: result.pending,
    inspectUserDataContext: result.context,
  };
}

// User-data import: an async operation (it compiles). The reply here is the terminal
// result; per-mod progress arrives separately as importUserDataProgress events (see
// useImportUserDataProgress), mirroring startUpdate + updateDownloadProgress.
export function useImportUserData<TContext extends Record<string, unknown>>(
  handler: (data: ImportUserDataReplyData, context?: TContext) => void
) {
  const selector = useCallback(
    (mockData: MockDataRegistry) => ({
      succeeded: true,
      summary: mockData.userDataImportSummary,
    }),
    []
  );
  const result = usePostMessageWithReplyWithMock<
    ImportUserDataData,
    ImportUserDataReplyData,
    TContext
  >('importUserData', handler, selector);
  return {
    importUserData: result.postMessage,
    importUserDataPending: result.pending,
    importUserDataContext: result.context,
  };
}

// Cancel an in-flight import (mirrors cancelUpdate); the import's own terminal reply
// still arrives with the summary of what completed before the cancel.
export function useCancelImportUserData<
  TContext extends Record<string, unknown>
>(handler: (data: CancelImportUserDataReplyData, context?: TContext) => void) {
  const result = usePostMessageWithReplyWithHandler<
    NoData,
    CancelImportUserDataReplyData,
    TContext
  >('cancelImportUserData', handler);
  return {
    cancelImportUserData: result.postMessage,
    cancelImportUserDataPending: result.pending,
    cancelImportUserDataContext: result.context,
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
