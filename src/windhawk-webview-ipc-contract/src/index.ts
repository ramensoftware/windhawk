// The webview IPC contract: the postMessage protocol between the Windhawk React
// webview and its hosts (the VSCode extension and the Tauri native shell). Both
// TypeScript hosts import the message payloads from here, and the Rust host
// mirrors them with typed serde structs proven against the shared fixture corpus
// in ./fixtures. The envelope those payloads travel in is the exception - see the
// note above webviewIPCMessageType.
//
// The webview is the superset consumer (it sees every host), so this file models
// the union of all hosts' messages; a given host implements the subset it needs.

/**
 * The contract version, asserted on the getInitialAppSettings handshake so a host
 * shipped against a different contract fails loudly instead of mis-handling a
 * message. Kept in lockstep with contract-version.json (a package test asserts
 * equality; the Rust host reads that JSON to check its own constant).
 */
export const WEBVIEW_IPC_CONTRACT_VERSION = '1.13.0';

/**
 * The machine-readable error a reply carries on a command failure (mirrors the
 * Rust reply error object). `code` is a stable SCREAMING_SNAKE string; `location`
 * is the optional source origin, shown human-facing only.
 *
 * Where a failure is SHOWN is each host's own, and the two answer it differently:
 * the native host reports nothing itself, so the webview's notification is the
 * report (its no-auto-surface list picks which codes reach it); the VSCode
 * extension pops a native notification from every catch it answers a request from,
 * so the webview leaves the telling to it there. What the object is FOR is the same
 * on both - what the failure means for the screen, e.g. a listing that stands for
 * less than it looks like or read-back fields that are stand-ins - so a consumer
 * reads it where it is there rather than counting on it: `error` is optional even
 * on a failure, attached always by the native host and where it carries something
 * by the VSCode extension.
 */
export type WireError = {
  code: string;
  message: string;
  /**
   * The failing resource (file path, registry key, or repo URL), when the error
   * names one - the most useful locus for an IO/registry/network failure.
   */
  path?: string;
  location?: { file: string; line: number };
};

/**
 * The theme setting: an explicit choice, or 'auto' to follow the host's light/dark
 * preference. Part of the contract because it rides AppSettings/AppUISettings and
 * the setNewAppSettings event.
 */
export type AppTheme = 'dark' | 'light' | 'auto';

// The envelope every payload travels in. Each host declares its own copy of it -
// windhawk-frontend's src/app/webviewIPC.ts, windhawk-vscode's src/webviewIPC.ts,
// windhawk-core's ui/src/ipc/envelope.rs - so a change made here alone reaches no
// host, and nothing compares the copies: a fixture is round-tripped through its
// `data`, and the version handshake passes on a framing mismatch. They are the
// shape to copy from, and what to import from once a host drops its own copy.

/**
 * Message types:
 * * 'message' is a message from the webview to the extension.
 * * 'messageWithReply' is a message from the webview to the extension that expects a reply.
 * * 'reply' is a reply to a 'messageWithReply' message.
 * * 'event' is a message from the extension to the webview.
 */
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
  /**
   * Whether update offers for this mod are suppressed, as a matcher over the
   * offered version rather than a flag. This comment is where the grammar is
   * explained for every mirror of the contract:
   *   ''        - updates are offered normally.
   *   '*'       - every offer for this mod is suppressed.
   *   '=1.2.3'  - the offer is suppressed while the available version is
   *               exactly 1.2.3, and returns as soon as it is anything else.
   *   anything else, INCLUDING a bare '=' - suppresses nothing.
   * That last row is deliberate: reads fail open, so a value this build does
   * not recognize costs the user an offer they see again rather than an update
   * withheld forever by a value no version can match. Writers are the strict
   * half of that split - see isValidSuppression below.
   */
  updatesDisabledForVersion: string;
};

/**
 * The 'suppress every offer' value of updatesDisabledForVersion, named so a
 * consumer building one does not spell the sentinel itself.
 */
export const ALL_VERSIONS = '*';

/**
 * What a stored updatesDisabledForVersion suppresses. Produced only by
 * parseSuppression, so a value of this type is one that came through the
 * grammar - and the union gives a consumer an exhaustive switch (which of the
 * two suppressions to name on a reenable affordance) instead of a
 * startsWith('=') test of its own.
 */
export type UpdateSuppression =
  | { readonly kind: 'all' }
  | { readonly kind: 'pinned'; readonly version: string };

/**
 * Decode a stored updatesDisabledForVersion. `null` is 'suppresses nothing',
 * which covers '', a bare '=' (a pin on the empty version, which no offer can
 * be), and every other value outside the grammar.
 */
export function parseSuppression(stored: string): UpdateSuppression | null {
  if (stored === ALL_VERSIONS) {
    return { kind: 'all' };
  }
  if (stored.startsWith('=') && stored.length > 1) {
    return { kind: 'pinned', version: stored.slice(1) };
  }
  return null;
}

/**
 * Encode a suppression as the value to store, the inverse of parseSuppression
 * (`parseSuppression(formatSuppression(s))` is `s` for every `s`). A writer
 * composes the union it already switches on rather than building a '=' prefix,
 * so the grammar has one implementation on the write side too; the result is
 * valid by construction, which is what isValidSuppression is left to check for
 * values that came from somewhere else. There is no encoding of 'updates are
 * on': that is the empty string, and a writer that means it says so.
 */
export function formatSuppression(suppression: UpdateSuppression): string {
  switch (suppression.kind) {
    case 'all':
      return ALL_VERSIONS;
    case 'pinned':
      return `=${suppression.version}`;
  }
}

/**
 * Whether a stored updatesDisabledForVersion suppresses an offer of `latest`.
 * The pin arm is equality, matching the host's own `latest !== installed`
 * update test: the suppression releases as soon as the offered version is
 * anything other than the pinned one.
 */
export function suppressesUpdateOffer(stored: string, latest: string): boolean {
  const suppression = parseSuppression(stored);
  if (suppression === null) {
    return false;
  }
  switch (suppression.kind) {
    case 'all':
      return true;
    case 'pinned':
      return suppression.version === latest;
  }
}

/**
 * Whether a value is one a WRITER may store: '' (updates on) or a value the
 * parser recognizes. The parser accepts anything and suppresses nothing for
 * what it does not recognize; this is the other half of that split, so a
 * writer cannot store a value that can never match - a '1.2.3' with the '='
 * forgotten would otherwise be stored, reported as a success, and honored by
 * nothing. The host enforces the same predicate on updateModConfig.
 */
export function isValidSuppression(value: string): boolean {
  return value === '' || parseSuppression(value) !== null;
}

export type AppSettings = {
  language: string;
  /**
   * Tauri only: the native shell persists the UI theme in the registry alongside
   * the other settings. Other backends (VSCode, website) do not send/return it and
   * the front-end keeps the theme in localStorage there, so it is optional.
   */
  theme?: AppTheme;
  disableUpdateCheck: boolean;
  disableRunUIScheduledTask: boolean | null;
  devModeOptOut: boolean;
  hideTrayIcon: boolean;
  alwaysCompileModsLocally: boolean;
  dontAutoShowToolkit: boolean;
  disableToolkitHotkey: boolean;
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

// The two things about an installed mod that only the user profile knows, which
/**
 * is why they can arrive apart from the mod itself: pushed by
 * updateInstalledModsDetails (which carries one more term beside them - see
 * UpdateInstalledModsDetailsEntry), and read back for the one mod an install or
 * a recompile touched.
 */
export type InstalledModProfileFields = {
  /**
   * The version the repository holds, as the host last cached it, or null where
   * it knows of none: updates were not checked for, the mod is local, or nothing
   * is cached. Not suppression-aware, so a refused offer still names what was
   * refused - which is what tells that state from a mod that is up to date.
   */
  latestVersion: string | null;
  userRating: number;
};

// Whether an update is on offer is
//   latestVersion !== null && latestVersion !== <installed metadata version>
//     && !suppressesUpdateOffer(config.updatesDisabledForVersion, latestVersion)
// which a consumer applies for itself over the mod it holds. The answer is not a
// field of its own: each of its three terms travels in a message that moves only
// that one - a config write turns an offer down, an install takes a version, a
// check caches a new one - so a consumer holding the conclusion would have to
// reach it again on each, and would show an offer that no longer stands wherever
// that was missed. `suppressesUpdateOffer` above is the half of the rule worth
// sharing; the rest is two comparisons.
//
// That reasoning holds only while every term does reach the consumer, and only
// one of the three does on its own: a check coming back IS
// updateInstalledModsDetails. The other two reach a consumer when it was the
// actor itself - an install it ran replies with the mod it landed, and
// setNewModConfig echoes a config write it asked for - and nothing tells it
// about either done in ANOTHER process. So the event carries all three
// (UpdateInstalledModsDetailsEntry): it fires exactly when another process has
// been at the mod, which is the one moment a consumer's own copy of the terms
// can be wrong.

/**
 * An installed mod, as the host reports one: what it is, how it is configured,
 * and the two things about it that only the user profile knows. The listings
 * carry the same set (with `metadata`/`config` nullable, a mod on disk the host
 * could not read); an install or a recompile reports it for the one mod it
 * touched, where both are known or the whole thing is null.
 */
export type InstalledModDetails = {
  metadata: ModMetadata;
  config: ModConfig;
} & InstalledModProfileFields;

export type AppUISettings = {
  language: string;
  /**
   * Tauri only: the initial theme the native shell delivers on startup and pushes on
   * every setNewAppSettings. Absent on other backends (the front-end uses localStorage).
   */
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

/**
 * The mod scope of a selection: a bare keyword, or an explicit id list. Serializes
 * as the string 'all' / 'all-except-local' / 'none', or the object { ids: [...] }.
 */
export type UserDataModScope =
  | 'all'
  | 'all-except-local'
  | 'none'
  | { ids: string[] };

/**
 * The per-mod facet toggles - whether to include a mod's runtime settings and its
 * user-owned config - used as the selection-wide `defaults`.
 */
export type UserDataFacetToggles = {
  settings: boolean;
  config: boolean;
};

/**
 * A per-mod override of the `defaults`: an omitted facet falls back to the default.
 */
export type UserDataPerModToggles = {
  settings?: boolean;
  config?: boolean;
};

/**
 * The granular selection, identical for export (what to include) and import (what
 * to apply, filtered by what the archive carries). `offline` is not here - it is a
 * per-direction option (see the export/import options below).
 */
export type UserDataSelection = {
  appSettings: boolean;
  mods: UserDataModScope;
  defaults: UserDataFacetToggles;
  perMod: Record<string, UserDataPerModToggles>;
};

/**
 * exportUserData options. `offline` embeds every repository mod's source so the
 * archive restores with no network (local mods always embed); off by default.
 */
export type UserDataExportOptions = {
  offline: boolean;
};

/**
 * importUserData options. `offline` demands a network-free restore (force local
 * compile, and refuse a reference-only mod with no embedded source); `noPrecompiled`
 * forces local compilation but may still fetch a reference-only mod's source;
 * `onConflict` decides how an already-installed mod is treated; `confirmAppRestart`
 * acknowledges that applying the archived app settings may require a restart.
 */
export type UserDataImportOptions = {
  offline: boolean;
  noPrecompiled: boolean;
  onConflict: 'overwrite' | 'skip';
  confirmAppRestart: boolean;
};

/**
 * One per-mod export warning (e.g. a mod whose source would not parse, so its
 * settings were omitted), named so the host can surface it.
 */
export type UserDataExportWarning = {
  modId: string;
  message: string;
};

/**
 * The export summary: per-mod warnings, empty on a clean export.
 */
export type UserDataExportSummary = {
  warnings: UserDataExportWarning[];
};

/**
 * One mod's row in the archive manifest: its identity plus which facets the archive
 * carries. `hasSource: false` marks a reference-only repository mod (its import
 * needs the network).
 */
export type UserDataManifestModEntry = {
  modId: string;
  isLocal: boolean;
  version: string;
  name: string | null;
  hasSource: boolean;
  hasSettings: boolean;
  hasConfig: boolean;
};

/**
 * The archive manifest inspectUserData projects: the metadata and per-mod
 * availability an import UI reads to build a selection over a specific archive.
 */
export type UserDataManifest = {
  exportedAt: string | null;
  hasAppSettings: boolean;
  mods: UserDataManifestModEntry[];
};

/**
 * One mod's terminal import outcome. `message` carries the failure reason for a
 * `failed` mod (and the skip reason for a `skipped` one); absent for `installed`.
 */
export type UserDataImportModOutcome = {
  modId: string;
  status: 'installed' | 'skipped' | 'failed';
  message?: string;
};

/**
 * The import summary: one outcome per processed mod, plus the app-settings
 * intents when app settings were applied (absent otherwise).
 */
export type UserDataImportSummary = {
  mods: UserDataImportModOutcome[];
  appSettings?: {
    requiresRestart: boolean;
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
  /**
   * The host's webview IPC contract version, asserted by the webview against
   * WEBVIEW_IPC_CONTRACT_VERSION on the bootstrap exchange (see the handshake).
   */
  contractVersion: string;
  appUISettings: Partial<AppUISettings>;
};

export type InstallModData = {
  modId: string;
  modSource: string;
  disabled?: boolean;
  loggingEnabled?: boolean;
};

/**
 * `installedModDetails` is the mod as the host lists it after the install, and
 * null where nothing landed. The profile-held fields ride along because the
 * install moves what they are about - a version arrives, so the update answer is
 * no longer the one the front-end was last told - and a front-end holding the
 * mod would otherwise have to invent them for one it did not have before.
 *
 * An install that lands names `latestVersion: null`, always: taking a version
 * from the repository is what makes the cached "latest" the version now
 * installed, so the host drops the cache and the repository side is unknown
 * again until the next check. A recompile takes no version, so a compileMod
 * reply DOES name one.
 *
 * `error` beside a NON-null details is the one partial success here: the mod is
 * on the machine and its metadata and config are what the operation did, but the
 * host could not read the mod back afterwards, so the profile-held fields are
 * stand-ins (null / 0) rather than answers. A consumer replacing an entry it
 * already had keeps its own two fields in that case rather than adopting these.
 * With no `error` every field is an answer.
 */
export type InstallModReplyData = {
  modId: string;
  installedModDetails: InstalledModDetails | null;
  uiMissing?: boolean;
  error?: WireError;
};

/**
 * cancelInstallMod: ask the host to stop the in-flight installMod for this mod,
 * whichever way it is installing (a precompiled download or a local compile).
 * It names a mod, unlike the bare cancelUpdate / cancelInstallDevTools: installs
 * for different mods run concurrently, so the command alone does not identify
 * one. The install's own reply still arrives, carrying installedModDetails: null.
 */
export type CancelInstallModData = {
  modId: string;
};

/**
 * `succeeded` is whether an in-flight install for `modId` was found and signaled.
 * false is the harmless no-op of a cancel that named a mod with nothing running -
 * including one whose install settled first (cancel races the terminal reply).
 */
export type CancelInstallModReplyData = {
  modId: string;
  succeeded: boolean;
};

export type CompileModData = {
  modId: string;
};

/**
 * `compiledModDetails` is what InstallModReplyData's `installedModDetails` is,
 * for the mod a recompile rebuilt - `error` included, on the same terms.
 */
export type CompileModReplyData = {
  modId: string;
  compiledModDetails: InstalledModDetails | null;
  uiMissing?: boolean;
  error?: WireError;
};

/**
 * cancelCompileMod: the recompile twin of cancelInstallMod. A recompile always
 * compiles locally, so this stops the compiler; the compileMod reply still
 * arrives, carrying compiledModDetails: null.
 */
export type CancelCompileModData = {
  modId: string;
};

/**
 * `succeeded` is whether an in-flight recompile for `modId` was found and
 * signaled; see CancelInstallModReplyData for what a false means.
 */
export type CancelCompileModReplyData = {
  modId: string;
  succeeded: boolean;
};

export type EnableModData = {
  modId: string;
  enable: boolean;
};

export type EnableModReplyData = {
  modId: string;
  enabled: boolean;
  succeeded: boolean;
  /**
   * Present only on failure: the standard error object the host attaches to the
   * reply (echo fields + succeeded:false + error). Absent on success.
   */
  error?: WireError;
};

export type DeleteModData = {
  modId: string;
};

export type DeleteModReplyData = {
  modId: string;
  succeeded: boolean;
  /**
   * Present only on failure: the standard error object the host attaches.
   */
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
  /**
   * Present only on failure: the standard error object the host attaches.
   */
  error?: WireError;
};

export type GetInstalledModsReplyData = {
  installedMods: Record<
    string,
    {
      metadata: ModMetadata | null;
      config: ModConfig | null;
    } & InstalledModProfileFields
  >;
  /**
   * Present when the listing is not the whole truth about the machine: the read
   * failed outright, and `installedMods` is then empty and stands for nothing
   * rather than for "no mods are installed"; or individual mods could not be
   * loaded and the map holds the rest. A consumer that only draws the listing can
   * ignore the distinction, but one that needs the COMPLETE set of installed ids
   * - to say what an import would overwrite, say - cannot take the map for it.
   */
  error?: WireError;
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
  /**
   * Present only on failure: the standard error object the host attaches.
   */
  error?: WireError;
};

export type GetModSettingsData = {
  modId: string;
};

export type GetModSettingsReplyData = {
  modId: string;
  settings: Record<string, string | number>;
  /**
   * Present only on failure: the standard error object the host attaches (the
   * base reply carries an empty settings map alongside it).
   */
  error?: WireError;
};

export type SetModSettingsData = {
  modId: string;
  settings: Record<string, string | number>;
};

export type SetModSettingsReplyData = {
  modId: string;
  succeeded: boolean;
  /**
   * Present only on failure: the standard error object the host attaches.
   */
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
  /**
   * Present only on failure: the standard error object the host attaches.
   */
  error?: WireError;
};

export type GetRepositoryModsReplyData = {
  mods: Record<
    string,
    {
      repository: {
        metadata: ModMetadata;
        /**
         * The mod as published, present on an entry the catalog serves
         * translated - `metadata` is then in the reader's language and this is
         * what it was translated from. Absent from the English catalog, and from
         * an entry no one has translated. A reader searching by the name they
         * saw elsewhere is searching in English, so a screen that filters offers
         * both.
         */
        metadataEnglish?: ModMetadata;
        details: RepositoryDetails;
        featured?: boolean;
      };
      /**
       * Present for a listed mod that is on the machine, carrying the same set
       * the installed listing does - `latestVersion` among them. A host builds
       * this side by joining that listing in, so the version the machine last
       * cached is already in its hands; a screen without it would work the
       * update answer out against the catalog's version instead, a different
       * cache of the same fact, and one mod could read two ways at once.
       */
      installed?: {
        metadata: ModMetadata | null;
        config: ModConfig | null;
      } & InstalledModProfileFields;
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

/**
 * The launch entry points (createNewMod / editMod / forkMod) reply so the native
 * UI can react: an empty object on success; { uiMissing: true } when the
 * development tools are not installed, which the front-end turns into the "install
 * development tools" modal; or the standard { error } object on any other failure,
 * which the IPC layer takes like any command error (see WireError) and reads as the
 * editor not opening.
 */
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

/**
 * deleteEditedMod: remove the copy of the edited mod on the machine, leaving the
 * source in the editor to be compiled again. The mod it names is the one the
 * editor session is on, like the other editor-mode commands, so nothing rides the
 * wire. The host answers with the details of what is left (setEditedModDetails).
 */
export type DeleteEditedModReplyData = {
  succeeded: boolean;
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

/**
 * exportUserData: the host calls the core with this selection, then opens a Save
 * dialog and writes the returned archive to the chosen file.
 */
export type ExportUserDataData = {
  selection: UserDataSelection;
  options: UserDataExportOptions;
};

/**
 * The export reply. `succeeded` is true once the archive was written; `canceled` is
 * true when the user dismissed the Save dialog (a benign no-op, no error surfaced).
 * `summary` carries the best-effort per-mod warnings on a successful export.
 */
export type ExportUserDataReplyData = {
  succeeded: boolean;
  summary?: UserDataExportSummary;
  canceled?: boolean;
  /**
   * Present only on failure: the standard error object the host attaches.
   */
  error?: WireError;
};

/**
 * inspectUserData: validate an archive and project its manifest. Without `archive`
 * the host owns the pick: it opens an Open dialog and reads the chosen file. With
 * `archive` the webview supplies the text itself (the user pasted it), so no dialog
 * runs and no file is read.
 */
export type InspectUserDataData = {
  archive?: string;
};

/**
 * The inspect reply. On success it carries the manifest and the archive bytes
 * themselves, so a subsequent importUserData can reuse them without a second read.
 * `canceled` marks a dismissed Open dialog.
 */
export type InspectUserDataReplyData = {
  succeeded: boolean;
  manifest?: UserDataManifest;
  archive?: string;
  canceled?: boolean;
  /**
   * Present only on failure (an unreadable file or an invalid archive).
   */
  error?: WireError;
};

/**
 * importUserData: an async operation (it compiles). The host drives the core import
 * over the archive the webview holds (from an earlier inspect) and forwards per-mod
 * progress as importUserDataProgress events; this reply is the terminal result.
 */
export type ImportUserDataData = {
  archive: string;
  selection: UserDataSelection;
  options: UserDataImportOptions;
};

/**
 * The import reply (terminal). `succeeded` is true when the operation completed -
 * even with per-mod failures, which the `summary` reports; inspect its per-mod
 * outcomes. On an operation-level failure or a cancellation `succeeded` is false,
 * and `error` may carry the failure object on the usual terms (see WireError) - the
 * one code to act on here is DEV_TOOLS_MISSING, which the import dialog turns into
 * the install prompt rather than a report.
 */
export type ImportUserDataReplyData = {
  succeeded: boolean;
  summary?: UserDataImportSummary;
  error?: WireError;
};

/**
 * cancelImportUserData: request the in-flight import stop (mirrors cancelUpdate);
 * the import's own terminal reply still arrives.
 */
export type CancelImportUserDataReplyData = {
  succeeded: boolean;
};

////////////////////////////////////////////////////////////
// Events.

export type SetNewAppSettingsData = {
  appUISettings: Partial<AppUISettings>;
};

export type UpdateDownloadProgressEventData = {
  /** 0-100 */
  progress: number;
};

export type UpdateInstallingEventData = NoData;

export type DevToolsInstallDownloadProgressEventData = {
  /** 0-100 */
  progress: number;
};

export type DevToolsInstallingEventData = NoData;

/**
 * One updateInstalledModsDetails entry: the profile-held pair, and the two terms
 * of the update rule that are fields of the MOD everywhere else this type's
 * siblings go. They are repeated here because this is the only one of them that
 * travels without the mod beside it, and a consumer applying the rule (see
 * InstalledModProfileFields) needs all three terms at once. The host has just
 * re-read the mod, so both are what the machine holds rather than the profile's
 * mirror of either.
 */
export type UpdateInstalledModsDetailsEntry = InstalledModProfileFields & {
  /**
   * The mod's own version, or null where the host could not read its source (the
   * same absence a listing reports as a null `metadata`). A consumer's copy of
   * this term is refreshed only by a full listing or by an operation the consumer
   * itself ran, so it is the term likeliest to be stale at the moment this event
   * arrives - an install or a recompile in another process is one of the things
   * that fires it.
   */
  installedVersion: string | null;
  updatesDisabledForVersion: ModConfig['updatesDisabledForVersion'];
};

export type UpdateInstalledModsDetailsData = {
  details: Record<string, UpdateInstalledModsDetailsEntry>;
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

/**
 * A per-mod `progress` marker importUserData emits as it works. Two shapes ride this
 * arm: a per-mod status marker (`status` set - the `installing` start and the terminal
 * `installed`/`skipped`/`failed`), and a forwarded install sub-event (`compileTarget`
 * set - a local compile's target). Both carry the `{ modId, index, total }`
 * position so the webview can render "mod 3 of 12" even for a precompiled install
 * that emits no sub-progress. `item` is the union discriminant, always 'mod' here.
 */
export type ImportUserDataModProgress = {
  item: 'mod';
  modId: string;
  index: number;
  total: number;
  status?: 'installing' | 'installed' | 'skipped' | 'failed';
  /**
   * The failure/skip reason on a terminal marker.
   */
  message?: string;
  /**
   * The target being compiled, on a forwarded local-compile sub-event.
   */
  compileTarget?: string;
};

/**
 * The app-settings step marker: `applying` as the import starts writing the archive's
 * global app settings, `applied` once done. Emitted once, before the mod loop, and
 * only when the import applies app settings - so it carries no `{ modId, index, total }`
 * mod position; it is a single step outside the mod count.
 */
export type ImportUserDataAppSettingsProgress = {
  item: 'appSettings';
  status: 'applying' | 'applied';
};

/**
 * A `progress` event importUserData emits as it works: a per-mod marker or the
 * app-settings step marker, discriminated by `item`.
 */
export type ImportUserDataProgressEventData =
  | ImportUserDataModProgress
  | ImportUserDataAppSettingsProgress;
