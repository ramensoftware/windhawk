export declare const WEBVIEW_IPC_CONTRACT_VERSION = "1.2.0";
export type WireError = {
    code: string;
    message: string;
    path?: string;
    location?: {
        file: string;
        line: number;
    };
};
export type AppTheme = 'dark' | 'light' | 'auto';
export type webviewIPCMessageType = 'message' | 'messageWithReply' | 'reply' | 'event';
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
export type webviewIPCMessageAny = webviewIPCMessage | webviewIPCMessageWithReply | webviewIPCReply | webviewIPCEvent;
export type NoData = Record<string, never>;
export type ModConfig = {
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
    theme?: AppTheme;
    disableUpdateCheck: boolean;
    disableRunUIScheduledTask: boolean | null;
    devModeOptOut: boolean;
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
    ratingBreakdown: number[];
    defaultSorting: number;
    published: number;
    updated: number;
};
export type AppUISettings = {
    language: string;
    theme?: AppTheme;
    devModeOptOut: boolean;
    loggingEnabled: boolean;
    updateIsAvailable: boolean;
    updateIsAvailableBleedingEdge: boolean;
    safeMode: boolean;
};
export type InitialSettingsValue = boolean | number | string | InitialSettings | InitialSettingsArrayValue;
export type InitialSettingsArrayValue = number[] | string[] | InitialSettings[];
export type InitialSettingItem = {
    key: string;
    value: InitialSettingsValue;
    name?: string;
    description?: string;
    options?: Record<string, string>[];
};
export type InitialSettings = InitialSettingItem[];
export type UserDataModScope = 'all' | 'all-except-local' | 'none' | {
    ids: string[];
};
export type UserDataFacetToggles = {
    settings: boolean;
    config: boolean;
};
export type UserDataPerModToggles = {
    settings?: boolean;
    config?: boolean;
};
export type UserDataSelection = {
    appSettings: boolean;
    mods: UserDataModScope;
    defaults: UserDataFacetToggles;
    perMod: Record<string, UserDataPerModToggles>;
};
export type UserDataExportOptions = {
    offline: boolean;
};
export type UserDataImportOptions = {
    offline: boolean;
    noPrecompiled: boolean;
    onConflict: 'overwrite' | 'skip';
    confirmAppRestart: boolean;
};
export type UserDataExportWarning = {
    modId: string;
    message: string;
};
export type UserDataExportSummary = {
    warnings: UserDataExportWarning[];
};
export type UserDataManifestModEntry = {
    modId: string;
    isLocal: boolean;
    version: string;
    name: string | null;
    hasSource: boolean;
    hasSettings: boolean;
    hasConfig: boolean;
};
export type UserDataManifest = {
    exportedAt: string | null;
    hasAppSettings: boolean;
    mods: UserDataManifestModEntry[];
};
export type UserDataImportModOutcome = {
    modId: string;
    status: 'installed' | 'skipped' | 'failed';
    message?: string;
};
export type UserDataImportSummary = {
    mods: UserDataImportModOutcome[];
    appSettings?: {
        requiresRestart: boolean;
        requiresNotify: boolean;
    };
};
export type EditModData = {
    modId: string;
};
export type ForkModData = {
    modId: string;
    modSource?: string;
};
export type GetInitialAppSettingsReplyData = {
    contractVersion: string;
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
    uiMissing?: boolean;
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
    uiMissing?: boolean;
};
export type EnableModData = {
    modId: string;
    enable: boolean;
};
export type EnableModReplyData = {
    modId: string;
    enabled: boolean;
    succeeded: boolean;
    error?: WireError;
};
export type DeleteModData = {
    modId: string;
};
export type DeleteModReplyData = {
    modId: string;
    succeeded: boolean;
    error?: WireError;
};
export type UpdateModRatingData = {
    modId: string;
    rating: number;
};
export type UpdateModRatingReplyData = {
    modId: string;
    rating: number;
    succeeded: boolean;
    error?: WireError;
};
export type GetInstalledModsReplyData = {
    installedMods: Record<string, {
        metadata: ModMetadata | null;
        config: ModConfig | null;
        updateAvailable: boolean;
        userRating: number;
    }>;
};
export type GetFeaturedModsReplyData = {
    featuredMods: Record<string, {
        metadata: ModMetadata;
        details: RepositoryDetails;
    }> | null;
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
    error?: WireError;
};
export type GetModSettingsData = {
    modId: string;
};
export type GetModSettingsReplyData = {
    modId: string;
    settings: Record<string, string | number>;
    error?: WireError;
};
export type SetModSettingsData = {
    modId: string;
    settings: Record<string, string | number>;
};
export type SetModSettingsReplyData = {
    modId: string;
    succeeded: boolean;
    error?: WireError;
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
    error?: WireError;
};
export type GetRepositoryModsReplyData = {
    mods: Record<string, {
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
    }> | null;
};
export type StartUpdateReplyData = {
    succeeded: boolean;
    error?: string;
};
export type CancelUpdateReplyData = {
    succeeded: boolean;
};
export type DevActionReplyData = {
    uiMissing?: boolean;
    error?: WireError;
};
export type StartInstallDevToolsReplyData = {
    succeeded: boolean;
    error?: string;
};
export type CancelInstallDevToolsReplyData = {
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
export type ExportUserDataData = {
    selection: UserDataSelection;
    options: UserDataExportOptions;
};
export type ExportUserDataReplyData = {
    succeeded: boolean;
    summary?: UserDataExportSummary;
    canceled?: boolean;
    error?: WireError;
};
export type InspectUserDataData = {
    archive?: string;
};
export type InspectUserDataReplyData = {
    succeeded: boolean;
    manifest?: UserDataManifest;
    archive?: string;
    canceled?: boolean;
    error?: WireError;
};
export type ImportUserDataData = {
    archive: string;
    selection: UserDataSelection;
    options: UserDataImportOptions;
};
export type ImportUserDataReplyData = {
    succeeded: boolean;
    summary?: UserDataImportSummary;
    error?: WireError;
};
export type CancelImportUserDataReplyData = {
    succeeded: boolean;
};
export type SetNewAppSettingsData = {
    appUISettings: Partial<AppUISettings>;
};
export type UpdateDownloadProgressEventData = {
    progress: number;
};
export type UpdateInstallingEventData = NoData;
export type DevToolsInstallDownloadProgressEventData = {
    progress: number;
};
export type DevToolsInstallingEventData = NoData;
export type UpdateInstalledModsDetailsData = {
    details: Record<string, {
        updateAvailable: boolean;
        userRating: number;
    }>;
};
export type SetNewModConfigData = {
    modId: string;
    config: Partial<ModConfig>;
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
export type ImportUserDataModProgress = {
    item: 'mod';
    modId: string;
    index: number;
    total: number;
    status?: 'installing' | 'installed' | 'skipped' | 'failed';
    message?: string;
    compileTarget?: string;
};
export type ImportUserDataAppSettingsProgress = {
    item: 'appSettings';
    status: 'applying' | 'applied';
};
export type ImportUserDataProgressEventData = ImportUserDataModProgress | ImportUserDataAppSettingsProgress;
