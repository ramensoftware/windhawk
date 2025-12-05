import * as vscode from 'vscode';
import {
  CancelUpdateReplyData,
  CompileEditedModReplyData,
  CompileModReplyData,
  DeleteModReplyData,
  EnableEditedModLoggingReplyData,
  EnableEditedModReplyData,
  EnableModReplyData,
  ExitEditorModeReplyData,
  GetAppSettingsReplyData,
  GetFeaturedModsReplyData,
  GetInitialAppSettingsReplyData,
  GetInstalledModsReplyData,
  GetModConfigReplyData,
  GetModSettingsReplyData,
  GetModSourceDataReplyData,
  GetModVersionsReplyData,
  GetRepositoryModSourceDataReplyData,
  GetRepositoryModsReplyData,
  InstallModReplyData,
  SetEditedModDetailsData,
  SetEditedModIdData,
  SetModSettingsReplyData,
  SetNewAppSettingsData,
  SetNewModConfigData,
  StartUpdateReplyData,
  UpdateAppSettingsReplyData,
  UpdateDownloadProgressEventData,
  UpdateInstalledModsDetailsData,
  UpdateInstallingEventData,
  UpdateModConfigReplyData,
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

// type MessageRegular = CommonMessageBase & {
//   type: 'message';
//   command: string;
//   data: Record<string, unknown>;
// };

// type MessageWithReply = CommonMessageBase & {
//   type: 'messageWithReply';
//   command: string;
//   data: Record<string, unknown>;
//   messageId: number;
// };

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

////////////////////////////////////////////////////////////
// Events.

export function setNewAppSettings(webview: vscode.Webview | undefined, data: SetNewAppSettingsData) {
  if (!webview) return;
  const msg: Event = {
    type: 'event',
    command: 'setNewAppSettings',
    data,
  };
  webview.postMessage(msg);
}

export function updateInstalledModsDetails(webview: vscode.Webview | undefined, data: UpdateInstalledModsDetailsData) {
  if (!webview) return;
  const msg: Event = {
    type: 'event',
    command: 'updateInstalledModsDetails',
    data,
  };
  webview.postMessage(msg);
}

export function setNewModConfig(webview: vscode.Webview | undefined, data: SetNewModConfigData) {
  if (!webview) return;
  const msg: Event = {
    type: 'event',
    command: 'setNewModConfig',
    data,
  };
  webview.postMessage(msg);
}

export function editedModWasModified(webview: vscode.Webview | undefined) {
  if (!webview) return;
  const msg: Event = {
    type: 'event',
    command: 'editedModWasModified',
    data: {},
  };
  webview.postMessage(msg);
}

export function compileEditedModStart(webview: vscode.Webview | undefined) {
  if (!webview) return;
  const msg: Event = {
    type: 'event',
    command: 'compileEditedModStart',
    data: {},
  };
  webview.postMessage(msg);
}

export function setEditedModDetails(webview: vscode.Webview | undefined, data: SetEditedModDetailsData) {
  if (!webview) return;
  const msg: Event = {
    type: 'event',
    command: 'setEditedModDetails',
    data,
  };
  webview.postMessage(msg);
}

export function setEditedModId(webview: vscode.Webview | undefined, data: SetEditedModIdData) {
  if (!webview) return;
  const msg: Event = {
    type: 'event',
    command: 'setEditedModId',
    data,
  };
  webview.postMessage(msg);
}

export function updateDownloadProgress(webview: vscode.Webview | undefined, data: UpdateDownloadProgressEventData) {
  if (!webview) return;
  const msg: Event = {
    type: 'event',
    command: 'updateDownloadProgress',
    data,
  };
  webview.postMessage(msg);
}

export function updateInstalling(webview: vscode.Webview | undefined, data: UpdateInstallingEventData) {
  if (!webview) return;
  const msg: Event = {
    type: 'event',
    command: 'updateInstalling',
    data,
  };
  webview.postMessage(msg);
}

////////////////////////////////////////////////////////////
// Replies.

export function getInitialAppSettingsReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: GetInitialAppSettingsReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'getInitialAppSettings',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function getInstalledModsReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: GetInstalledModsReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'getInstalledMods',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function getFeaturedModsReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: GetFeaturedModsReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'getFeaturedMods',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function getRepositoryModsReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: GetRepositoryModsReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'getRepositoryMods',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function getModSourceDataReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: GetModSourceDataReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'getModSourceData',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function getRepositoryModSourceDataReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: GetRepositoryModSourceDataReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'getRepositoryModSourceData',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function getModVersionsReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: GetModVersionsReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'getModVersions',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function getModSettingsReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: GetModSettingsReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'getModSettings',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function setModSettingsReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: SetModSettingsReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'setModSettings',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function getModConfigReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: GetModConfigReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'getModConfig',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function updateModConfigReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: UpdateModConfigReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'updateModConfig',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function installModReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: InstallModReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'installMod',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function compileModReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: CompileModReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'compileMod',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function enableModReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: EnableModReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'enableMod',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function deleteModReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: DeleteModReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'deleteMod',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function updateModRatingReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: UpdateModRatingReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'updateModRating',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function getAppSettingsReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: GetAppSettingsReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'getAppSettings',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function updateAppSettingsReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: UpdateAppSettingsReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'updateAppSettings',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function enableEditedModReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: EnableEditedModReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'enableEditedMod',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function enableEditedModLoggingReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: EnableEditedModLoggingReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'enableEditedModLogging',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function compileEditedModReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: CompileEditedModReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'compileEditedMod',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function exitEditorModeReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: ExitEditorModeReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'exitEditorMode',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function startUpdateReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: StartUpdateReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'startUpdate',
    messageId,
    data,
  };
  webview.postMessage(msg);
}

export function cancelUpdateReply(
  webview: vscode.Webview | undefined,
  messageId: number,
  data: CancelUpdateReplyData
) {
  if (!webview) return;
  const msg: Reply = {
    type: 'reply',
    command: 'cancelUpdate',
    messageId,
    data,
  };
  webview.postMessage(msg);
}
