import { useCallback, useEffect, useRef, useState } from 'react';

import backendApi from './backendApi';
import { promptDevToolsInstall } from './devToolsInstall';
import { isWireError, surfaceWireError } from './feedback';
import {
  type CancelInstallDevToolsReplyData,
  type CancelUpdateReplyData,
  type CompileEditedModData,
  type CompileEditedModReplyData,
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
 * React hook wrapper for sendMessageWithReply that manages pending state and context.
 * Handles race conditions by canceling old requests when new ones arrive or on unmount.
 */
function usePostMessageWithReplyWithHandler<
  TPostMessage extends Record<string, unknown>,
  TReply,
  TContext extends Record<string, unknown>
>(eventName: string, handler: (data: TReply, context?: TContext) => void) {
  if (WEBPACK_IS_WEBSITE) {
    throw new Error(
      `usePostMessageWithReplyWithHandler("${eventName}") must not be called in browser mode`
    );
  }

  const [pending, setPending] = useState(false);
  const [context, setContext] = useState<TContext>();
  const cancelRef = useRef<(() => void) | null>(null);
  const requestIdRef = useRef(0);
  const handlerRef = useRef<typeof handler>();

  // Keep handler ref up to date
  useEffect(() => {
    handlerRef.current = handler;
  }, [handler]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (cancelRef.current) {
        cancelRef.current();
        cancelRef.current = null;
      }
    };
  }, []);

  const postMessage = useCallback(
    async (data: TPostMessage, ctx?: TContext) => {
      // Cancel any previous pending request
      if (cancelRef.current) {
        console.warn(`Cancelling previous pending request for event "${eventName}"`);
        cancelRef.current();
        cancelRef.current = null;
      }

      // Generate a unique ID for this request to detect stale responses
      const currentRequestId = ++requestIdRef.current;

      setPending(true);
      setContext(ctx);

      const { promise, cancel } = sendMessageWithReply<TPostMessage, TReply>(
        eventName,
        data
      );
      cancelRef.current = cancel;

      try {
        const reply = await promise;

        // Only process the reply if this is still the current request
        if (currentRequestId === requestIdRef.current) {
          // Centrally surface a command failure: every reply funnels through here,
          // so a handler that swallows the error into its command-specific default
          // (null / empty / succeeded:false) still gets the failure shown once. The
          // handler still runs (it owns the data-shape side of the failure). Only the
          // standard error OBJECT triggers this; startUpdate's error STRING is left
          // to its own modal.
          const replyError = (reply as { error?: unknown }).error;
          if (isWireError(replyError)) {
            surfaceWireError(replyError);
          }
          handlerRef.current?.(reply, ctx);
        }
      } catch (error) {
        // Don't throw MessageCancelledError - cancellation is expected behavior
        if (!(error instanceof MessageCancelledError)) {
          throw error;
        }
      } finally {
        // Only cleanup state if this is still the current request
        if (currentRequestId === requestIdRef.current) {
          setPending(false);
          setContext(undefined);
          cancelRef.current = null;
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
 * @returns Same interface as usePostMessageWithReplyWithHandler
 */
function usePostMessageWithReplyWithMockDev<
  TPostMessage extends Record<string, unknown>,
  TReply,
  TContext extends Record<string, unknown>
>(
  eventName: string,
  handler: (data: TReply, context?: TContext) => void,
  mockDataSelector?: (mockData: MockDataRegistry, request: TPostMessage) => TReply
) {
  const { isMockMode, mockData } = useMockContext();

  // Always call the real hook to maintain hook order (even if we won't use it)
  const realResult = usePostMessageWithReplyWithHandler<
    TPostMessage,
    TReply,
    TContext
  >(eventName, handler);

  // Mock mode: create a simulated IPC call (always create it to maintain hook order)
  const mockPostMessage = useCallback(
    (data: TPostMessage, ctx?: TContext) => {
      // Simulate async behavior
      if (mockDataSelector) {
        setTimeout(() => {
          const mockReply = mockDataSelector(mockData, data);
          handler(mockReply, ctx);
        }, 0);
      }
    },
    [handler, mockData, mockDataSelector]
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
  _mockDataSelector?: unknown
) {
  return usePostMessageWithReplyWithHandler<TPostMessage, TReply, TContext>(
    eventName,
    handler
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
    (mockData: MockDataRegistry) => ({ appUISettings: mockData.appUISettings }),
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
  const result = usePostMessageWithReplyWithHandler<
    InstallModData,
    InstallModReplyData,
    TContext
  >('installMod', handler);
  return {
    installMod: result.postMessage,
    installModPending: result.pending,
    installModContext: result.context,
  };
}

export function useCompileMod<TContext extends Record<string, unknown>>(
  handler: (data: CompileModReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    CompileModData,
    CompileModReplyData,
    TContext
  >('compileMod', handler);
  return {
    compileMod: result.postMessage,
    compileModPending: result.pending,
    compileModContext: result.context,
  };
}

export function useEnableMod<TContext extends Record<string, unknown>>(
  handler: (data: EnableModReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    EnableModData,
    EnableModReplyData,
    TContext
  >('enableMod', handler);
  return {
    enableMod: result.postMessage,
    enableModPending: result.pending,
    enableModContext: result.context,
  };
}

export function useDeleteMod<TContext extends Record<string, unknown>>(
  handler: (data: DeleteModReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    DeleteModData,
    DeleteModReplyData,
    TContext
  >('deleteMod', handler);
  return {
    deleteMod: result.postMessage,
    deleteModPending: result.pending,
    deleteModContext: result.context,
  };
}

export function useUpdateModRating<TContext extends Record<string, unknown>>(
  handler: (data: UpdateModRatingReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    UpdateModRatingData,
    UpdateModRatingReplyData,
    TContext
  >('updateModRating', handler);
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
  >('getModSourceData', handler, selector);
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
  const result = usePostMessageWithReplyWithHandler<
    GetRepositoryModSourceDataData,
    GetRepositoryModSourceDataReplyData,
    TContext
  >('getRepositoryModSourceData', handler);
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
  >('getModVersions', handler, selector);
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
  const result = usePostMessageWithReplyWithHandler<
    UpdateAppSettingsData,
    UpdateAppSettingsReplyData,
    TContext
  >('updateAppSettings', handler);
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
  >('getModSettings', handler, selector);
  return {
    getModSettings: result.postMessage,
    getModSettingsPending: result.pending,
    getModSettingsContext: result.context,
  };
}

export function useSetModSettings<TContext extends Record<string, unknown>>(
  handler: (data: SetModSettingsReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    SetModSettingsData,
    SetModSettingsReplyData,
    TContext
  >('setModSettings', handler);
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
  >('getModConfig', handler, selector);
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
  >('updateModConfig', handler, selector);
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
