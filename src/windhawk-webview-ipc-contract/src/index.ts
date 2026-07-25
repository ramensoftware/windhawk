// The webview IPC contract: the postMessage protocol between the Windhawk React
// webview and its hosts (the VSCode extension and the Tauri native shell). This
// package is the single source of truth for that protocol; both TypeScript hosts
// import it, and the Rust host mirrors it with typed serde structs proven against
// the shared fixture corpus in ./fixtures.
//
// The webview is the superset consumer (it sees every host), so this file models
// the union of all hosts' messages; a given host implements the subset it needs.

// The contract version, asserted on the getInitialAppSettings handshake so a host
// shipped against a different contract fails loudly instead of mis-handling a
// message. Kept in lockstep with contract-version.json (a package test asserts
// equality; the Rust host reads that JSON to check its own constant).
export const WEBVIEW_IPC_CONTRACT_VERSION = '1.2.0';

// The machine-readable error a reply carries on a command failure (mirrors the
// Rust reply error object). `code` is a stable SCREAMING_SNAKE string; `location`
// is the optional source origin, shown human-facing only.
export type WireError = {
  code: string;
  message: string;
  // The failing resource (file path, registry key, or repo URL), when the error
  // names one - the most useful locus for an IO/registry/network failure.
  path?: string;
  location?: { file: string; line: number };
};

// The theme setting: an explicit choice, or 'auto' to follow the host's light/dark
// preference. Part of the contract because it rides AppSettings/AppUISettings and
// the setNewAppSettings event.
export type AppTheme = 'dark' | 'light' | 'auto';

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
  // Tauri only: the native shell persists the UI theme in the registry alongside
  // the other settings. Other backends (VSCode, website) do not send/return it and
  // the front-end keeps the theme in localStorage there, so it is optional.
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
  // Tauri only: the initial theme the native shell delivers on startup and pushes on
  // every setNewAppSettings. Absent on other backends (the front-end uses localStorage).
  theme?: AppTheme;
  devModeOptOut: boolean;
  loggingEnabled: boolean;
  updateIsAvailable: boolean;
  updateIsAvailableBleedingEdge: boolean;
  safeMode: boolean;
};

export type InitialSettingsValue =
  | boolean
  | number
  | string
  | InitialSettings
  | InitialSettingsArrayValue;

export type InitialSettingsArrayValue = number[] | string[] | InitialSettings[];

export type InitialSettingItem = {
  key: string;
  value: InitialSettingsValue;
  name?: string;
  description?: string;
  options?: Record<string, string>[];
};

export type InitialSettings = InitialSettingItem[];

// User-data export/import (the `data` feature). The archive bytes cross the wire as
// an opaque string (the core owns the format); these types model the selection, the
// options, and the projected manifest/summaries a host and the webview exchange.

// The mod scope of a selection: a bare keyword, or an explicit id list. Serializes
// as the string 'all' / 'all-except-local' / 'none', or the object { ids: [...] }.
export type UserDataModScope =
  | 'all'
  | 'all-except-local'
  | 'none'
  | { ids: string[] };

// The per-mod facet toggles - whether to include a mod's runtime settings and its
// user-owned config - used as the selection-wide `defaults`.
export type UserDataFacetToggles = {
  settings: boolean;
  config: boolean;
};

// A per-mod override of the `defaults`: an omitted facet falls back to the default.
export type UserDataPerModToggles = {
  settings?: boolean;
  config?: boolean;
};

// The granular selection, identical for export (what to include) and import (what
// to apply, filtered by what the archive carries). `offline` is not here - it is a
// per-direction option (see the export/import options below).
export type UserDataSelection = {
  appSettings: boolean;
  mods: UserDataModScope;
  defaults: UserDataFacetToggles;
  perMod: Record<string, UserDataPerModToggles>;
};

// exportUserData options. `offline` embeds every repository mod's source so the
// archive restores with no network (local mods always embed); off by default.
export type UserDataExportOptions = {
  offline: boolean;
};

// importUserData options. `offline` demands a network-free restore (force local
// compile, and refuse a reference-only mod with no embedded source); `noPrecompiled`
// forces local compilation but may still fetch a reference-only mod's source;
// `onConflict` decides how an already-installed mod is treated; `confirmAppRestart`
// acknowledges that applying the archived app settings may require a restart.
export type UserDataImportOptions = {
  offline: boolean;
  noPrecompiled: boolean;
  onConflict: 'overwrite' | 'skip';
  confirmAppRestart: boolean;
};

// One per-mod export warning (e.g. a mod whose source would not parse, so its
// settings were omitted), named so the host can surface it.
export type UserDataExportWarning = {
  modId: string;
  message: string;
};

// The export summary: per-mod warnings, empty on a clean export.
export type UserDataExportSummary = {
  warnings: UserDataExportWarning[];
};

// One mod's row in the archive manifest: its identity plus which facets the archive
// carries. `hasSource: false` marks a reference-only repository mod (its import
// needs the network).
export type UserDataManifestModEntry = {
  modId: string;
  isLocal: boolean;
  version: string;
  name: string | null;
  hasSource: boolean;
  hasSettings: boolean;
  hasConfig: boolean;
};

// The archive manifest inspectUserData projects: the metadata and per-mod
// availability an import UI reads to build a selection over a specific archive.
export type UserDataManifest = {
  exportedAt: string | null;
  hasAppSettings: boolean;
  mods: UserDataManifestModEntry[];
};

// One mod's terminal import outcome. `message` carries the failure reason for a
// `failed` mod (and the skip reason for a `skipped` one); absent for `installed`.
export type UserDataImportModOutcome = {
  modId: string;
  status: 'installed' | 'skipped' | 'failed';
  message?: string;
};

// The import summary: one outcome per processed mod, plus the app-settings
// restart/notify intents when app settings were applied (absent otherwise).
export type UserDataImportSummary = {
  mods: UserDataImportModOutcome[];
  appSettings?: {
    requiresRestart: boolean;
    requiresNotify: boolean;
  };
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
  // The host's webview IPC contract version, asserted by the webview against
  // WEBVIEW_IPC_CONTRACT_VERSION on the bootstrap exchange (see the handshake).
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
  // Present only on failure: the standard error object the host attaches to the
  // reply (echo fields + succeeded:false + error). Absent on success.
  error?: WireError;
};

export type DeleteModData = {
  modId: string;
};

export type DeleteModReplyData = {
  modId: string;
  succeeded: boolean;
  // Present only on failure: the standard error object the host attaches.
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
  // Present only on failure: the standard error object the host attaches.
  error?: WireError;
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
  // Present only on failure: the standard error object the host attaches.
  error?: WireError;
};

export type GetModSettingsData = {
  modId: string;
};

export type GetModSettingsReplyData = {
  modId: string;
  settings: Record<string, string | number>;
  // Present only on failure: the standard error object the host attaches (the
  // base reply carries an empty settings map alongside it).
  error?: WireError;
};

export type SetModSettingsData = {
  modId: string;
  settings: Record<string, string | number>;
};

export type SetModSettingsReplyData = {
  modId: string;
  succeeded: boolean;
  // Present only on failure: the standard error object the host attaches.
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
  // Present only on failure: the standard error object the host attaches.
  error?: WireError;
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

// The launch entry points (createNewMod / editMod / forkMod) reply so the native
// UI can react: an empty object on success; { uiMissing: true } when the
// development tools are not installed, which the front-end turns into the "install
// development tools" modal; or the standard { error } object on any other failure,
// which the IPC layer auto-surfaces like any command error.
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

// User-data export/import. The host owns the file dialogs and the archive file I/O;
// the core owns the archive bytes and the transaction. So the webview asks the host
// to export/inspect/import and the host runs the native Save/Open picker around the
// core call.

// exportUserData: the host calls the core with this selection, then opens a Save
// dialog and writes the returned archive to the chosen file.
export type ExportUserDataData = {
  selection: UserDataSelection;
  options: UserDataExportOptions;
};

// The export reply. `succeeded` is true once the archive was written; `canceled` is
// true when the user dismissed the Save dialog (a benign no-op, no error surfaced).
// `summary` carries the best-effort per-mod warnings on a successful export.
export type ExportUserDataReplyData = {
  succeeded: boolean;
  summary?: UserDataExportSummary;
  canceled?: boolean;
  // Present only on failure: the standard error object the host attaches.
  error?: WireError;
};

// inspectUserData: validate an archive and project its manifest. Without `archive`
// the host owns the pick: it opens an Open dialog and reads the chosen file. With
// `archive` the webview supplies the text itself (the user pasted it), so no dialog
// runs and no file is read.
export type InspectUserDataData = {
  archive?: string;
};

// The inspect reply. On success it carries the manifest and the archive bytes
// themselves, so a subsequent importUserData can reuse them without a second read.
// `canceled` marks a dismissed Open dialog.
export type InspectUserDataReplyData = {
  succeeded: boolean;
  manifest?: UserDataManifest;
  archive?: string;
  canceled?: boolean;
  // Present only on failure (an unreadable file or an invalid archive).
  error?: WireError;
};

// importUserData: an async operation (it compiles). The host drives the core import
// over the archive the webview holds (from an earlier inspect) and forwards per-mod
// progress as importUserDataProgress events; this reply is the terminal result.
export type ImportUserDataData = {
  archive: string;
  selection: UserDataSelection;
  options: UserDataImportOptions;
};

// The import reply (terminal). `succeeded` is true when the operation completed -
// even with per-mod failures, which the `summary` reports; inspect its per-mod
// outcomes. On an operation-level failure or a cancellation `succeeded` is false;
// `error` MAY carry the failure object, per the host's surfacing policy - the
// native host attaches it always (the webview's no-auto-surface list decides what
// to show), while the VSCode extension surfaces failures through its own
// notifications and attaches only the codes the webview must react to
// programmatically (e.g. DEV_TOOLS_MISSING). Consumers treat `error` as optional
// even on failure.
export type ImportUserDataReplyData = {
  succeeded: boolean;
  summary?: UserDataImportSummary;
  error?: WireError;
};

// cancelImportUserData: request the in-flight import stop (mirrors cancelUpdate);
// the import's own terminal reply still arrives.
export type CancelImportUserDataReplyData = {
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

export type DevToolsInstallDownloadProgressEventData = {
  progress: number; // 0-100
};

export type DevToolsInstallingEventData = NoData;

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

// A per-mod `progress` marker importUserData emits as it works. Two shapes ride this
// arm: a per-mod status marker (`status` set - the `installing` start and the terminal
// `installed`/`skipped`/`failed`), and a forwarded install sub-event (`compileTarget`
// set - a local compile's target). Both carry the `{ modId, index, total }`
// position so the webview can render "mod 3 of 12" even for a precompiled install
// that emits no sub-progress. `item` is the union discriminant, always 'mod' here.
export type ImportUserDataModProgress = {
  item: 'mod';
  modId: string;
  index: number;
  total: number;
  status?: 'installing' | 'installed' | 'skipped' | 'failed';
  // The failure/skip reason on a terminal marker.
  message?: string;
  // The target being compiled, on a forwarded local-compile sub-event.
  compileTarget?: string;
};

// The app-settings step marker: `applying` as the import starts writing the archive's
// global app settings, `applied` once done. Emitted once, before the mod loop, and
// only when the import applies app settings - so it carries no `{ modId, index, total }`
// mod position; it is a single step outside the mod count.
export type ImportUserDataAppSettingsProgress = {
  item: 'appSettings';
  status: 'applying' | 'applied';
};

// A `progress` event importUserData emits as it works: a per-mod marker or the
// app-settings step marker, discriminated by `item`.
export type ImportUserDataProgressEventData =
  | ImportUserDataModProgress
  | ImportUserDataAppSettingsProgress;
