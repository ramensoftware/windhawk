// Message types:
// * 'message' is a message from the webview to the extension.
// * 'messageWithReply' is a message from the webview to the extension that expects a reply.
// * 'reply' is a reply to a 'messageWithReply' message.
// * 'event' is a message from the extension to the webview.
export type webviewIPCMessageType =
  | 'message'
  | 'messageWithReply'
  | 'reply'
  | 'event';

export type webviewIPCMessageCommon = {
  type: webviewIPCMessageType;
  command: string;
  data: Record<string, unknown>;
};

export type webviewIPCMessage = webviewIPCMessageCommon & {
  type: 'message';
  command: string;
  data: Record<string, unknown>;
};

export type webviewIPCMessageWithReply = webviewIPCMessageCommon & {
  type: 'messageWithReply';
  command: string;
  data: Record<string, unknown>;
  messageId: number;
};

export type webviewIPCReply = webviewIPCMessageCommon & {
  type: 'reply';
  command: string;
  data: Record<string, unknown>;
  messageId: number;
};

export type webviewIPCEvent = webviewIPCMessageCommon & {
  type: 'event';
  command: string;
  data: Record<string, unknown>;
};

export type webviewIPCMessageAny =
  | webviewIPCMessage
  | webviewIPCMessageWithReply
  | webviewIPCReply
  | webviewIPCEvent;

////////////////////////////////////////////////////////////
// Types.

export type NoData = Record<string, never>;

// Shared data shapes are part of the core contract so the IPC layer, the
// front-ends, and the core agree on them. Imported here for use in the
// message definitions below; consumers should import these types from
// coreClient/contract directly.
import {
  AppSettings,
  AppUISettings,
  InitialSettings,
  ModConfig,
  ModMetadata,
  RepositoryDetails,
} from './coreClient/contract';

////////////////////////////////////////////////////////////
// Messages.

export type EditModData = {
  modId: string;
};

export type ForkModData = {
  modId: string;
  modSource?: string;
};

////////////////////////////////////////////////////////////
// Messages with replies.

export type GetInitialAppSettingsReplyData = {
  appUISettings: Partial<AppUISettings>;
};

export type InstallModData = {
  modId: string;
  modSource: string;
  disabled?: boolean;
  loggingEnabled?: boolean;
};

export type InstallModReplyData = {
  modId: string;
  installedModDetails: {
    metadata: ModMetadata;
    config: ModConfig;
  } | null;
};

export type CompileModData = {
  modId: string;
};

export type CompileModReplyData = {
  modId: string;
  compiledModDetails: {
    metadata: ModMetadata;
    config: ModConfig;
  } | null;
};

export type EnableModData = {
  modId: string;
  enable: boolean;
};

export type EnableModReplyData = {
  modId: string;
  enabled: boolean;
  succeeded: boolean;
};

export type DeleteModData = {
  modId: string;
};

export type DeleteModReplyData = {
  modId: string;
  succeeded: boolean;
};

export type UpdateModRatingData = {
  modId: string;
  rating: number;
};

export type UpdateModRatingReplyData = {
  modId: string;
  rating: number;
  succeeded: boolean;
};

export type GetInstalledModsReplyData = {
  installedMods: Record<
    string,
    {
      metadata: ModMetadata | null;
      config: ModConfig | null;
      updateAvailable: boolean;
      userRating: number;
    }
  >;
};

export type GetFeaturedModsReplyData = {
  featuredMods: Record<
    string,
    {
      metadata: ModMetadata;
      details: RepositoryDetails;
    }
  > | null;
};

export type GetModSourceDataData = {
  modId: string;
};

export type GetModSourceDataReplyData = {
  modId: string;
  data: {
    source: string | null;
    metadata: ModMetadata | null;
    readme: string | null;
    initialSettings: InitialSettings | null;
  };
};

export type GetRepositoryModSourceDataData = {
  modId: string;
  version?: string;
};

export type GetRepositoryModSourceDataReplyData = {
  modId: string;
  version?: string;
  data: {
    source: string | null;
    metadata: ModMetadata | null;
    readme: string | null;
    initialSettings: InitialSettings | null;
  };
};

export type GetModVersionsData = {
  modId: string;
};

export type GetModVersionsReplyData = {
  modId: string;
  versions: {
    version: string;
    timestamp: number;
    isPreRelease: boolean;
  }[];
};

export type GetAppSettingsReplyData = {
  appSettings: Partial<AppSettings>;
};

export type UpdateAppSettingsData = {
  appSettings: Partial<AppSettings>;
};

export type UpdateAppSettingsReplyData = {
  appSettings: Partial<AppSettings>;
  succeeded: boolean;
};

export type GetModSettingsData = {
  modId: string;
};

export type GetModSettingsReplyData = {
  modId: string;
  settings: Record<string, string | number>;
};

export type SetModSettingsData = {
  modId: string;
  settings: Record<string, string | number>;
};

export type SetModSettingsReplyData = {
  modId: string;
  succeeded: boolean;
};

export type GetModConfigData = {
  modId: string;
};

export type GetModConfigReplyData = {
  modId: string;
  config: ModConfig | null;
};

export type UpdateModConfigData = {
  modId: string;
  config: Partial<ModConfig>;
};

export type UpdateModConfigReplyData = {
  modId: string;
  succeeded: boolean;
};

export type GetRepositoryModsReplyData = {
  mods: Record<
    string,
    {
      repository: {
        metadata: ModMetadata;
        details: RepositoryDetails;
        featured?: boolean;
      };
      installed?: {
        metadata: ModMetadata | null;
        config: ModConfig | null;
        userRating: number;
      };
    }
  > | null;
};

export type StartUpdateReplyData = {
  succeeded: boolean;
  error?: string;
};

export type CancelUpdateReplyData = {
  succeeded: boolean;
};

// The reply the launch entry points (createNewMod / editMod / forkMod) send. In this
// (in-VSCodium) UI the code editor is always present, so uiMissing never occurs here;
// the handler replies an empty object on success or a standard { error } object on
// failure, which the webview auto-surfaces.
export type DevActionReplyData = {
  uiMissing?: boolean;
  error?: {
    code: string;
    message: string;
  };
};

export type EnableEditedModData = {
  enable: boolean;
};

export type EnableEditedModReplyData = {
  enabled: boolean;
  succeeded: boolean;
};

export type EnableEditedModLoggingData = {
  enable: boolean;
};

export type EnableEditedModLoggingReplyData = {
  enabled: boolean;
  succeeded: boolean;
};

export type CompileEditedModData = {
  disabled?: boolean;
  loggingEnabled?: boolean;
};

export type CompileEditedModReplyData = {
  succeeded: boolean;
  clearModified: boolean;
};

export type ExitEditorModeData = {
  saveToDrafts: boolean;
};

export type ExitEditorModeReplyData = {
  succeeded: boolean;
};

////////////////////////////////////////////////////////////
// Events.

export type SetNewAppSettingsData = {
  appUISettings: Partial<AppUISettings>;
};

export type UpdateDownloadProgressEventData = {
  progress: number; // 0-100
};

export type UpdateInstallingEventData = NoData;

export type UpdateInstalledModsDetailsData = {
  details: Record<
    string,
    {
      updateAvailable: boolean;
      userRating: number;
    }
  >;
};

export type SetNewModConfigData = {
  modId: string,
  config: Partial<ModConfig>
};

export type SetEditedModIdData = {
  modId: string;
};

export type SetEditedModDetailsData = {
  modId: string;
  modDetails: ModConfig | null;
  modWasModified: boolean;
  noWindhawkExitButton: boolean;
};
