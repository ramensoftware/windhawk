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

export type NoData = Record<string, unknown>;

export type ModConfig = {
  // libraryFileName: string;
  disabled: boolean;
  loggingEnabled: boolean;
  debugLoggingEnabled: boolean;
  include: string[];
  exclude: string[];
  includeCustom: string[];
  excludeCustom: string[];
  includeExcludeCustomOnly: boolean;
  patternsMatchCriticalSystemProcesses: boolean;
  architecture: string[];
  version: string;
};

export type AppSettings = {
  language: string;
  disableUpdateCheck: boolean;
  disableRunUIScheduledTask: boolean | null;
  devModeOptOut: boolean;
  devModeUsedAtLeastOnce: boolean;
  hideTrayIcon: boolean;
  alwaysCompileModsLocally: boolean;
  dontAutoShowToolkit: boolean;
  modTasksDialogDelay: number;
  safeMode: boolean;
  loggingVerbosity: number;
  engine: {
    loggingVerbosity: number;
    include: string[];
    exclude: string[];
    injectIntoCriticalProcesses: boolean;
    injectIntoIncompatiblePrograms: boolean;
    injectIntoGames: boolean;
  };
};

export type ModMetadata = Partial<{
  version: string;
  // id: string;
  github: string;
  twitter: string;
  homepage: string;
  compilerOptions: string;
  license: string;
  donateUrl: string;
  name: string;
  description: string;
  author: string;
  include: string[];
  exclude: string[];
  architecture: string[];
}>;

export type RepositoryDetails = {
  users: number;
  rating: number;
  // ratingUsers: number;
  ratingBreakdown: number[];
  defaultSorting: number;
  published: number;
  updated: number;
};

export type AppUISettings = {
  language: string;
  devModeOptOut: boolean;
  devModeUsedAtLeastOnce: boolean;
  loggingEnabled: boolean;
  updateIsAvailable: boolean;
  safeMode: boolean;
};

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
  disabled?: boolean;
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
    initialSettings: Record<string, any>[] | null;
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
    initialSettings: Record<string, any>[] | null;
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
  settings: Record<string, any>;
};

export type SetModSettingsData = {
  modId: string;
  settings: Record<string, any>;
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
  disabled: boolean;
  loggingEnabled: boolean;
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
};
