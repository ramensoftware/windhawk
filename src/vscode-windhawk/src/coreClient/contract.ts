// The Windhawk core contract: every command, DTO, event shape, and error
// class the two front-ends (the VSCode extension and windhawk-cli) consume from
// the shared core. This module is the TypeScript source of truth for the
// windhawk-core command inventory; the Rust core mirrors these shapes 1:1 with
// serde.
//
// The contract seam: the only backend is the in-process TypeScript
// one (src/coreClient/inProcessBackend.ts), which delegates to the existing
// services in src/services/. The data shapes themselves still live in
// src/services/types.ts (the services need them too) and are re-exported
// here; front-ends import them from this module only. When the TypeScript
// backend is deleted, the definitions move here.
//
// Error model: commands reject with the same error objects the
// services throw today, so front-end error handling is byte-identical.
// The typed classes (WindhawkError subclasses, CompilerError) are part of
// the contract and re-exported below. Untyped failures (e.g. Node ENOENT errors
// from getModSource) cross the contract unchanged; the native
// backend will map them to the stable error codes of the core error model.

import type {
	AppSettings,
	Catalog,
	InitialSettings,
	ModConfig,
	ModMetadata,
	ModSettings,
	ModVersionInfo,
} from '../services/types';
import type {
	AppSettingsIntents,
	CatalogForProfileSync,
	CompileInstalledModInput,
	CompileInstalledModResult,
	InstallModResult,
} from '../services/operations';

// Bumped on breaking contract changes; reported by getCoreInfo and asserted
// by the client when the native backend arrives.
export const CONTRACT_VERSION = '0.1.0';

// Data shapes shared with the webview IPC layer and the services.
export type {
	AppSettings,
	AppUISettings,
	Catalog,
	CatalogEntry,
	InitialSettingItem,
	InitialSettings,
	InitialSettingsArrayValue,
	InitialSettingsValue,
	ModConfig,
	ModMetadata,
	ModSettings,
	ModVersionInfo,
	RepositoryDetails,
} from '../services/types';

// Operation DTOs (single-sourced with the operations layer).
export type {
	AppSettingsIntents,
	CatalogForProfileSync,
	CompileInstalledModInput,
	CompileInstalledModResult,
	InstallModResult,
} from '../services/operations';

// The user-facing message channel: the host injects its implementation
// (VSCode notifications, CLI stderr) when creating the core.
export type { Logger } from '../services/logger';

// Typed errors. Runtime re-exports so `instanceof` works across the
// boundary: the in-process backend throws the very same classes.
export { ModNotInRepoError, RepoUnreachableError, WindhawkError } from '../services/errors';
export { CompilerError, CompilerKilled } from '../services/compiler';
export type { UpdateProgress } from '../services/update';

////////////////////////////////////////////////////////////
// Command DTOs.

export type CoreFsPaths = {
	appRootPath: string;
	appDataPath: string;
	enginePath: string;
	compilerPath: string;
	uiPath: string;
};

export type CoreInfo = {
	contractVersion: string;
	portable: boolean;
	arm64Enabled: boolean;
	// Raw installed-Windhawk version string as provided by the host; null
	// when unknown.
	windhawkVersion: string | null;
	fsPaths: CoreFsPaths;
};

// Result of parseModSource. Each section is parsed independently so one
// malformed block doesn't hide the others (the GUI shows per-section errors
// and still renders what parsed). A section with an `errors` entry has a
// null value; a null value without an error means the section is absent
// (readme/initialSettings) or, for metadata, that parsing failed.
export type ParsedModSource = {
	metadata: ModMetadata | null;
	readme: string | null;
	initialSettings: InitialSettings | null;
	errors: {
		metadata?: string;
		readme?: string;
		initialSettings?: string;
	};
};

export type ListInstalledModsParams = {
	// Language for localized metadata extraction.
	language: string;
	// When false, no mod reports an available update (mirrors the GUI's
	// disableUpdateCheck gate). The value is a parameter, not an internal
	// read, so each front-end keeps its current sourcing (the GUI a cached
	// value, the CLI a fresh settings read).
	checkForUpdates: boolean;
	// When true, reconcile the user profile with the installed state
	// (per-mod version/disabled refresh, removed-mod cleanup) and persist it
	// if anything changed. The GUI's installed-mods query and the CLI's
	// `mod list` sync; pure listings (profile-change refresh, repo-list
	// installed-state decoration) don't.
	syncProfile: boolean;
};

export type InstalledModListEntry = {
	metadata: ModMetadata | null;
	config: ModConfig | null;
	updateAvailable: boolean;
	userRating: number;
};

export type ListInstalledModsResult = {
	mods: Record<string, InstalledModListEntry>;
	// Mods whose source failed to parse, with the failure rendered as
	// `String(error)`. Surfacing is front-end policy (GUI notification,
	// CLI stderr warning).
	loadErrors: { modId: string; error: string }[];
};

// Same field set as the operations-layer InstallModInput minus
// modsFolderUrl: the repository folder URL for precompiled downloads is
// core-internal knowledge (the front-ends no longer know repository URLs).
export type CoreInstallModInput = {
	// Storage id under which the mod is persisted: the bare repo id, or
	// local@<id> for locally-authored mods (file installs, editor mods).
	storageId: string;
	// Mod source code, CRLF-normalized by the caller.
	source: string;
	// Metadata already extracted from `source` and validated by the caller
	// (id present and reconciled against storageId).
	metadata: ModMetadata;
	// true/false sets the state explicitly; omitted preserves the existing
	// state (which on a fresh install means the backend's default of
	// enabled).
	disabled?: boolean;
	// Same omitted-means-preserve semantics as `disabled`.
	loggingEnabled?: boolean;
	// The compile-vs-download decision, made by the caller: the GUI uses its
	// cached alwaysCompileModsLocally setting, the CLI combines a fresh
	// settings read with its --file / --no-precompiled flags.
	compileLocally: boolean;
	// false for local@ mods, which are not tracked in the user profile.
	trackInProfile: boolean;
	// Editor compile only: precompiled-headers folder passed through to the
	// compiler.
	pchFolder?: string;
	// Editor compile only: when the mod id was renamed in the source, the
	// previous storage id whose config is moved to storageId (after the
	// compile succeeds) and whose source file is deleted (after the new
	// source is written).
	renameFromStorageId?: string;
};

export type AppUpdateStatus = {
	// Raw cached latest-version strings from the user profile; null when
	// not recorded. latestVersion is the grace-period-filtered value behind
	// the GUI's update badge; latestVersionBleedingEdge is the raw latest.
	latestVersion: string | null;
	latestVersionBleedingEdge: string | null;
	// Comparisons against the session's installed Windhawk version.
	updateAvailable: boolean;
	updateAvailableBleedingEdge: boolean;
};

export type ProfileWatchInfo = {
	// Absolute path of userprofile.json, for the extension's external-change
	// watcher.
	filePath: string;
	// mtime of the last profile write this session performed on the user's
	// behalf (as opposed to external updates); null if none yet. The watcher
	// uses it to ignore its own writes.
	lastModifiedByUserMtimeMs: number | null;
};

export type TrayAction = 'restartBg' | 'newUpdatesFound' | 'appSettingsChanged';

////////////////////////////////////////////////////////////
// Async operations and events.

// Handle for a long-running command. cancel() is synchronous and
// cooperative (mirrors WhCoreCancel): it signals the operation, which then
// terminates with a cancellation error (CompilerKilled for compiles, an
// abort for update downloads); cancel of a finished operation is a harmless
// no-op. The in-process backend implements cancel via today's global
// cancelCompilation/cancelUpdate semantics, which are equivalent while at
// most one compile/update runs per session.
export interface AsyncOperation<T> {
	readonly result: Promise<T>;
	cancel(): boolean;
}

// Events of startUpdate; reproduces exactly what the front-ends consume
// today (download percentage and the "installing" transition).
export interface UpdateEvents {
	onProgress: (data: { progress: number }) => void;
	onInstalling: () => void;
}

////////////////////////////////////////////////////////////
// The core interface.

// The single surface through which both front-ends access the shared core. One
// method per command of the windhawk-core command inventory. Every method is
// async (Promise or AsyncOperation): the future DLL-backed client cannot offer
// synchronous calls, so the seam is async from day one.
export interface WindhawkCore {
	// --- Meta ---

	getCoreInfo(): Promise<CoreInfo>;

	// --- Pure helpers (no storage I/O) ---

	// Parse metadata, readme, and initial settings out of mod source code.
	parseModSource(source: string, language: string): Promise<ParsedModSource>;

	// Append suffixes to the @id and @name metadata fields of a mod source
	// (new-mod and fork flows). Returns the transformed source.
	appendToModIdAndName(source: string, appendToId?: string, appendToName?: string): Promise<string>;

	// --- Installed-mod queries and scoped writes ---

	// Composite installed-mods listing: source metadata, config,
	// profile-derived updateAvailable/userRating, and (optionally) the
	// profile reconciliation both front-ends perform.
	listInstalledMods(params: ListInstalledModsParams): Promise<ListInstalledModsResult>;

	// Stored source for a mod id. Rejects with the raw filesystem
	// error (ENOENT errno) when the source file is missing, exactly like
	// today's service call; callers map it to their own error types.
	getModSource(modId: string): Promise<string>;

	// Whether a mod occupies a storage id (source file or config entry).
	doesModExist(modId: string): Promise<boolean>;

	getModConfig(modId: string): Promise<ModConfig | null>;

	// Patch semantics: absent fields are preserved.
	updateModConfig(modId: string, patch: Partial<ModConfig>): Promise<void>;

	getModSettings(modId: string): Promise<ModSettings>;

	setModSettings(modId: string, settings: ModSettings): Promise<void>;

	// Scoped single-field write used by the editor sidebar's logging toggle.
	setModLoggingEnabled(modId: string, enable: boolean): Promise<void>;

	// User-profile write (rating of 0 clears the entry).
	setModRating(modId: string, rating: number): Promise<void>;

	// --- Use-case operations ---

	// Install or reinstall a mod from already-extracted source + metadata:
	// migrate mod settings, compile locally or download a precompiled DLL,
	// persist config and source, clean up DLLs of prior versions, and
	// record the version in the user profile. Also serves the editor
	// compile flow via pchFolder and renameFromStorageId. Source
	// acquisition and metadata validation are the caller's job;
	// CompilerError propagates unchanged through result.
	installMod(input: CoreInstallModInput): AsyncOperation<InstallModResult>;

	// Recompile an already-installed mod from its stored source: only the
	// library file name and the metadata-derived config fields get updated.
	compileInstalledMod(input: CompileInstalledModInput): AsyncOperation<CompileInstalledModResult>;

	// Enable or disable an installed mod, mirroring the new state into the
	// user profile for non-local mods. Callers own any existence or
	// already-in-state checks; this always writes.
	setModEnabled(modId: string, enable: boolean): Promise<void>;

	// Uninstall a mod: config, source, DLLs, and (for non-local mods)
	// profile entry. The extension's editor-draft cleanup stays in the
	// front-end.
	removeMod(modId: string): Promise<void>;

	// Apply an app-settings patch and return what the change demands of the
	// tray program. Always writes; tray policy is decided by the caller
	// from the returned intents.
	applyAppSettings(patch: Partial<AppSettings>): Promise<AppSettingsIntents>;

	// Pure predicate over a patch: what applying it WOULD demand. Used by
	// the CLI's pre-write --confirm-app-restart gate.
	previewAppSettingsEffects(patch: Partial<AppSettings>): Promise<AppSettingsIntents>;

	// Record the catalog's latest app and per-mod versions in the user
	// profile. Returns whether the profile changed; posting the tray's
	// "new updates found" notification from that is front-end policy.
	syncCatalogToProfile(catalog: CatalogForProfileSync): Promise<{ profileUpdated: boolean }>;

	// --- App settings ---

	getAppSettings(): Promise<AppSettings>;

	// --- Repository (network) ---

	// Language-specific catalog with default-catalog fallback on 404.
	fetchCatalog(language: string): Promise<Catalog>;

	// Mod source at an optional version, CRLF-normalized. 404 rejects with
	// ModNotInRepoError; other failures with RepoUnreachableError.
	fetchRepoModSource(modId: string, version?: string): Promise<string>;

	// Parsed versions.json. Same error mapping as fetchRepoModSource.
	fetchModVersions(modId: string): Promise<ModVersionInfo[]>;

	// --- User profile auxiliary ---

	// Cached latest-version state and its comparison against the installed
	// version (the GUI's update badge, the CLI's `update status`).
	getAppUpdateStatus(): Promise<AppUpdateStatus>;

	// Profile file path + last-own-write mtime for the extension's
	// external-change watcher.
	getProfileWatchInfo(): Promise<ProfileWatchInfo>;

	// --- Tray (mechanism only; when to call is front-end policy) ---

	notifyTray(action: TrayAction): Promise<void>;

	// --- Update ---

	// Download the Windhawk installer (progress events), then launch it
	// detached ("installing" event). Rejects with an error if an update is
	// already in flight. Cancel via the returned handle.
	startUpdate(events: UpdateEvents): AsyncOperation<void>;

	// --- Editor support ---

	// The clangd flag set for the mod-editing workspace's compile_flags.txt,
	// single-sourced in the core with the real compiler flags (an additive
	// command). Pure: no params, no session I/O.
	getCompileFlags(): Promise<string[]>;
}
