import { useCallback, useState } from 'react';
import { useEventListener } from 'usehooks-ts';

import vsCodeApi from './vsCodeApi';
import {
  CancelUpdateReplyData,
  CompileEditedModData,
  CompileEditedModReplyData,
  CompileModData,
  CompileModReplyData,
  DeleteModData,
  DeleteModReplyData,
  EditModData,
  EnableEditedModData,
  EnableEditedModLoggingData,
  EnableEditedModLoggingReplyData,
  EnableEditedModReplyData,
  EnableModData,
  EnableModReplyData,
  ExitEditorModeData,
  ExitEditorModeReplyData,
  ForkModData,
  GetAppSettingsReplyData,
  GetFeaturedModsReplyData,
  GetInitialAppSettingsReplyData,
  GetInstalledModsReplyData,
  GetModConfigData,
  GetModConfigReplyData,
  GetModSettingsData,
  GetModSettingsReplyData,
  GetModSourceDataData,
  GetModSourceDataReplyData,
  GetModVersionsData,
  GetModVersionsReplyData,
  GetRepositoryModSourceDataData,
  GetRepositoryModSourceDataReplyData,
  GetRepositoryModsReplyData,
  InstallModData,
  InstallModReplyData,
  NoData,
  SetEditedModDetailsData,
  SetEditedModIdData,
  SetModSettingsData,
  SetModSettingsReplyData,
  SetNewAppSettingsData,
  SetNewModConfigData,
  StartUpdateReplyData,
  UpdateAppSettingsData,
  UpdateAppSettingsReplyData,
  UpdateDownloadProgressEventData,
  UpdateInstalledModsDetailsData,
  UpdateInstallingEventData,
  UpdateModConfigData,
  UpdateModConfigReplyData,
  UpdateModRatingData,
  UpdateModRatingReplyData
} from './webviewIPCMessages';

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

export function createNewMod() {
  const msg: MessageRegular = {
    type: 'message',
    command: 'createNewMod',
    data: {},
  };
  vsCodeApi?.postMessage(msg);
}

export function editMod(data: EditModData) {
  const msg: MessageRegular = {
    type: 'message',
    command: 'editMod',
    data,
  };
  vsCodeApi?.postMessage(msg);
}

export function forkMod(data: ForkModData) {
  const msg: MessageRegular = {
    type: 'message',
    command: 'forkMod',
    data,
  };
  vsCodeApi?.postMessage(msg);
}

export function showAdvancedDebugLogOutput() {
  const msg: MessageRegular = {
    type: 'message',
    command: 'showAdvancedDebugLogOutput',
    data: {},
  };
  vsCodeApi?.postMessage(msg);
}

export function showLogOutput() {
  const msg: MessageRegular = {
    type: 'message',
    command: 'showLogOutput',
    data: {},
  };
  vsCodeApi?.postMessage(msg);
}

export function getInitialSidebarParams() {
  const msg: MessageRegular = {
    type: 'message',
    command: 'getInitialSidebarParams',
    data: {},
  };
  vsCodeApi?.postMessage(msg);
}

export function stopCompileEditedMod() {
  const msg: MessageRegular = {
    type: 'message',
    command: 'stopCompileEditedMod',
    data: {},
  };
  vsCodeApi?.postMessage(msg);
}

export function previewEditedMod() {
  const msg: MessageRegular = {
    type: 'message',
    command: 'previewEditedMod',
    data: {},
  };
  vsCodeApi?.postMessage(msg);
}

////////////////////////////////////////////////////////////
// Messages with replies.

let messageId = 0;

function usePostMessageWithReplyWithHandler<
  TPostMessage extends Record<string, unknown>,
  TReply,
  TContext extends Record<string, unknown>
>(eventName: string, handler: (data: TReply, context?: TContext) => void) {
  const [pendingMessageId, setPendingMessageId] = useState<number>();
  const [context, setContext] = useState<TContext>();

  const postMessage = useCallback(
    (data: TPostMessage, context?: TContext) => {
      messageId++;
      if (messageId > 0x7fffffff) {
        messageId = 1;
      }

      const message: MessageWithReply = {
        type: 'messageWithReply',
        command: eventName,
        data,
        messageId,
      };
      vsCodeApi?.postMessage(message);

      setPendingMessageId(messageId);
      setContext(context);
    },
    [eventName]
  );

  useEventListener(
    'message',
    useCallback(
      (message) => {
        const data = message.data as MessageAny;

        if (pendingMessageId === undefined) {
          return;
        }

        if (
          data.type === 'reply' &&
          data.command === eventName &&
          data.messageId === pendingMessageId
        ) {
          handler(data.data as TReply, context);
          setPendingMessageId(undefined);
          setContext(undefined);
        }
      },
      [context, eventName, handler, pendingMessageId]
    )
  );

  return { postMessage, pending: pendingMessageId !== undefined, context };
}

export function useGetInitialAppSettings<
  TContext extends Record<string, unknown>
>(handler: (data: GetInitialAppSettingsReplyData, context?: TContext) => void) {
  const result = usePostMessageWithReplyWithHandler<
    NoData,
    GetInitialAppSettingsReplyData,
    TContext
  >('getInitialAppSettings', handler);
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
  const result = usePostMessageWithReplyWithHandler<
    NoData,
    GetInstalledModsReplyData,
    TContext
  >('getInstalledMods', handler);
  return {
    getInstalledMods: result.postMessage,
    getInstalledModsPending: result.pending,
    getInstalledModsContext: result.context,
  };
}

export function useGetFeaturedMods<TContext extends Record<string, unknown>>(
  handler: (data: GetFeaturedModsReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    NoData,
    GetFeaturedModsReplyData,
    TContext
  >('getFeaturedMods', handler);
  return {
    getFeaturedMods: result.postMessage,
    getFeaturedModsPending: result.pending,
    getFeaturedModsContext: result.context,
  };
}

export function useGetModSourceData<TContext extends Record<string, unknown>>(
  handler: (data: GetModSourceDataReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    GetModSourceDataData,
    GetModSourceDataReplyData,
    TContext
  >('getModSourceData', handler);
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
  const result = usePostMessageWithReplyWithHandler<
    GetModVersionsData,
    GetModVersionsReplyData,
    TContext
  >('getModVersions', handler);
  return {
    getModVersions: result.postMessage,
    getModVersionsPending: result.pending,
    getModVersionsContext: result.context,
  };
}

export function useGetAppSettings<TContext extends Record<string, unknown>>(
  handler: (data: GetAppSettingsReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    NoData,
    GetAppSettingsReplyData,
    TContext
  >('getAppSettings', handler);
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
  const result = usePostMessageWithReplyWithHandler<
    GetModSettingsData,
    GetModSettingsReplyData,
    TContext
  >('getModSettings', handler);
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
  const result = usePostMessageWithReplyWithHandler<
    GetModConfigData,
    GetModConfigReplyData,
    TContext
  >('getModConfig', handler);
  return {
    getModConfig: result.postMessage,
    getModConfigPending: result.pending,
    getModConfigContext: result.context,
  };
}

export function useUpdateModConfig<TContext extends Record<string, unknown>>(
  handler: (data: UpdateModConfigReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    UpdateModConfigData,
    UpdateModConfigReplyData,
    TContext
  >('updateModConfig', handler);
  return {
    updateModConfig: result.postMessage,
    updateModConfigPending: result.pending,
    updateModConfigContext: result.context,
  };
}

export function useGetRepositoryMods<TContext extends Record<string, unknown>>(
  handler: (data: GetRepositoryModsReplyData, context?: TContext) => void
) {
  const result = usePostMessageWithReplyWithHandler<
    NoData,
    GetRepositoryModsReplyData,
    TContext
  >('getRepositoryMods', handler);
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

function useEventMessageWithHandler<T>(
  eventName: string,
  handler: (data: T) => void
) {
  useEventListener(
    'message',
    useCallback(
      (message) => {
        const data = message.data as MessageAny;
        if (data.type === 'event' && data.command === eventName) {
          handler(data.data as T);
        }
      },
      [eventName, handler]
    )
  );
}

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
  useEventMessageWithHandler<SetEditedModDetailsData>(
    'setEditedModDetails',
    handler
  );
}
