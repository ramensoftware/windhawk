// The Windhawk core contract: every command, DTO, event shape, and error
// class the front-end (the VSCode extension) consumes from the shared core.
// This module is the TypeScript source of truth for the windhawk-core command
// inventory; the Rust core (the windhawk-core repository) mirrors these shapes
// 1:1 with serde.
//
// The only backend is windhawk-core.dll, reached through the napi bridge
// (dllBackend.ts). Data shapes live here (the webview IPC layer imports them
// from this module); the typed error classes live in ./errors and are
// re-exported below so `instanceof` works across the boundary (the DLL backend
// rebuilds the same classes from the wire error envelope).

// Bumped on breaking contract changes; reported by getCoreInfo and asserted by
// the client at session creation (dllBackend.ts). The DLL must report the same
// value or the client refuses to load it - there is no in-process fallback, so
// the DLL and this client are version-locked.
export const CONTRACT_VERSION = '0.1.0';

// The user-facing message channel: the host injects its implementation (VSCode
// notifications, CLI stderr) when creating the core.
export type { Logger } from './logger';

// Typed errors. Runtime re-exports so `instanceof` works in front-end error
// handling; the DLL backend throws these very classes.
export {
	CompilerError,
	CompilerKilled,
	ModNotInRepoError,
	RepoUnreachableError,
	WindhawkError,
} from './errors';
export type { CompilationTarget } from './errors';

////////////////////////////////////////////////////////////
// Shared data shapes (also the IPC contract the React webview syncs against).

export type ModConfig = {
	libraryFileName: string;
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
	// Which update offers the user has refused for this mod: '' none, '*' every
	// one, '=<version>' that one. The grammar and the predicates over it live in
	// the webview IPC contract package (parseSuppression, suppressesUpdateOffer,
	// isValidSuppression); the core enforces it on updateModConfig. Nothing here
	// interprets the value - the extension passes it between the DLL and the
	// webview - so it is typed as the string it is on the wire.
	updatesDisabledForVersion: string;
	// Which configuration of the mod is in use for the session's Windows user,
	// in their programs and in what the core reads and writes. Read-only:
	// updateModConfig refuses a patch naming it, and setModUserScope /
	// deleteModUserConfig change it.
	userScope: UserScope;
	// Why the Windhawk mod catalog took the mod off every machine, or '' when it
	// did not. A nuked mod does not run whatever `disabled` says, and enabling
	// it is refused with MOD_NUKED. Read-only: the catalog sync, the update check
	// and an install of the offered version change it.
	nuked: string;
};

// The ModConfig fields updateModConfig writes. The core refuses a patch naming
// the read-only userScope or nuked; they are `never` rather than left out so
// that a whole config spread into a patch fails to compile as well.
export type ModConfigPatch = Partial<Omit<ModConfig, 'userScope' | 'nuked'>> & {
	userScope?: never;
	nuked?: never;
};

// 'none': the user has no configuration of their own. 'inactive': they have
// one, switched off and kept, and the global one is in use. 'active': their own
// is in use.
export type UserScope = 'none' | 'inactive' | 'active';

export type AppSettings = {
	language: string;
	disableUpdateCheck: boolean;
	// null in portable mode (the scheduled task only exists in non-portable installs).
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
	id: string;
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
	trendingSorting?: number;
	published: number;
	updated: number;
	// Why the repository took versions of the mod off every machine, when it did:
	// a reason for every version, or reasons keyed by version for the versions
	// they name. The core passes the catalog through untouched, and the webview
	// reads this.
	nuked?: string | Record<string, string>;
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
	// $format: a display hint the core forwards as the mod wrote it and never
	// interprets; the webview draws a control for the formats it knows.
	format?: string;
	// $float: value is a decimal held as a string (or a string array). true or
	// absent, never false, so an older reader sees a plain string setting.
	float?: true;
	// $dynamicSelect: the mod supplies dropdown options at runtime (see
	// getModDynamicSelectOptions). true or absent, like float.
	dynamicSelect?: true;
	// $min / $max: bounds on a number item (a number value, or a $float one),
	// each the literal as the mod wrote it; present only when declared.
	min?: number;
	max?: number;
	// $showIf / $hideIf: the settings the item's visibility depends on, each by
	// the declaration path the core resolved it to (dotted, no array subscripts)
	// mapped to the values under which the entry holds. An editor hint the
	// webview evaluates; present only when declared.
	showIf?: SettingConditions;
	hideIf?: SettingConditions;
};

export type InitialSettings = InitialSettingItem[];

// The entries of one $showIf / $hideIf map.
export type SettingConditions = Record<string, (boolean | number | string)[]>;

// Per-mod runtime settings, stored as a flat key/value map. Nested/array source
// declarations are flattened at write time by the core.
export type ModSettings = Record<string, string | number>;

// One option of a $dynamicSelect setting: what a selection stores and the text
// the dropdown shows.
export type DynamicSelectOption = {
	value: string;
	label: string;
};

// The options a mod wrote at runtime for its $dynamicSelect settings, keyed by
// setting path (the flat key with every [n] removed). Paths and entries are in
// the order the mod wrote them; the core does not sort.
export type ModDynamicSelectOptions = Record<string, DynamicSelectOption[]>;

// One upvote the user cast on a review (a review or a reply) of a repository
// mod: the review's server id and when the vote was cast, in unix seconds.
// Kept in the user profile as a [timestamp, reviewId] pair, where it is the
// "voted" mark and the re-vote guard; the app's update check delivers it.
export type ModReviewVote = {
	reviewId: number;
	timestamp: number;
};

// UI-bootstrap subset surfaced to the webview; derived from AppSettings plus
// update/user-profile state.
export type AppUISettings = {
	language: string;
	devModeOptOut: boolean;
	loggingEnabled: boolean;
	updateIsAvailable: boolean;
	updateIsAvailableBleedingEdge: boolean;
	safeMode: boolean;
};

// One mod's entry in the repository catalog JSON.
export type CatalogEntry = {
	metadata: ModMetadata;
	// The mod as published, on an entry a language catalog serves translated:
	// `metadata` is then in that language and this is what it was translated
	// from. Absent from catalog.json, and from an entry no one has translated.
	// The webview searches over both, so it is forwarded rather than dropped.
	metadataEnglish?: ModMetadata;
	details: RepositoryDetails;
	featured?: boolean;
};

// The repository catalog (catalogs/<language>.json / catalog.json).
export type Catalog = {
	app: { version?: string; versionBleedingEdge?: string };
	mods: Record<string, CatalogEntry>;
};

// One entry of a mod's versions.json, normalized for consumers (isPreRelease is
// derived from the version string).
export type ModVersionInfo = {
	version: string;
	timestamp: number;
	isPreRelease: boolean;
};

////////////////////////////////////////////////////////////
// Use-case ("operations") DTOs.

export type AppSettingsIntents = {
	requiresRestart: boolean;
};

// Minimal structural slice of the repository catalog JSON that the profile sync
// needs. Both the extension's raw catalog response and the typed Catalog
// satisfy it.
export type CatalogForProfileSync = {
	app: {
		version?: string;
		versionBleedingEdge?: string;
	};
	mods: Record<string, {
		metadata: {
			version?: string;
		};
	}>;
};

export type CompileInstalledModInput = {
	// Storage id of the installed mod (bare id, or local@<id>).
	storageId: string;
	// The mod's stored source code, read by the caller.
	source: string;
	// Metadata already extracted from `source` and validated by the caller
	// (id present and reconciled against storageId modulo the local@ prefix).
	metadata: ModMetadata;
};

export type CompileInstalledModResult = {
	// The mod's config as read back from storage after the compile.
	config: ModConfig;
	targetDllName: string;
	// The clang diagnostics of a successful compile (warnings emitted even
	// though the mod compiled), tagged per target. Absent/empty on a clean
	// compile; the front-end shows a non-empty value in the compiler output.
	warnings?: string;
};

export type InstallModResult = {
	// The mod's config as read back from storage after the install.
	config: ModConfig;
	targetDllName: string;
	// The clang diagnostics of a successful local compile, tagged per target.
	// Absent/empty on a clean compile or a precompiled download; the front-end
	// shows a non-empty value in the compiler output.
	warnings?: string;
};

// Update download progress event payload.
export type UpdateProgress = {
	progress: number; // 0-100
};

////////////////////////////////////////////////////////////
// User-data export/import DTOs.

// The largest archive the core accepts, mirroring MAX_ARCHIVE_BYTES in the Rust
// contract. An archive that arrives as a FILE is the host's to read, so it is
// sized against this before the read: an oversized document is refused without
// ever being pulled into memory, whereas the core can only reject it once it
// holds the whole string.
export const MAX_ARCHIVE_BYTES = 64 * 1024 * 1024;

// The mod scope of a selection: a keyword, or an explicit id list.
export type UserDataModScope = 'all' | 'all-except-local' | 'none' | { ids: string[] };

// The per-mod facet toggles (runtime settings and user-owned config).
export type UserDataFacetToggles = {
	settings: boolean;
	config: boolean;
};

// A per-mod override of the defaults; an omitted facet falls back to the default.
export type UserDataPerModToggles = {
	settings?: boolean;
	config?: boolean;
};

// The granular selection, identical for export (what to include) and import (what
// to apply). offline is a per-direction option, not part of the shared selection.
export type UserDataSelection = {
	appSettings: boolean;
	mods: UserDataModScope;
	defaults: UserDataFacetToggles;
	perMod: Record<string, UserDataPerModToggles>;
};

export type UserDataExportOptions = {
	// Embed every repository mod's source so the archive restores with no network.
	offline: boolean;
};

export type UserDataImportOptions = {
	// Require a network-free restore (refuse a reference-only mod, force local compile).
	offline: boolean;
	// Force local compilation (may still fetch a reference-only mod's source).
	noPrecompiled: boolean;
	onConflict: 'overwrite' | 'skip';
	// Acknowledge that applying the archived app settings may require a restart.
	confirmAppRestart: boolean;
};

export type UserDataExportWarning = {
	modId: string;
	message: string;
};

// The export summary: per-mod warnings, empty on a clean export.
export type UserDataExportSummary = {
	warnings: UserDataExportWarning[];
};

export type ExportUserDataInput = {
	selection: UserDataSelection;
	options: UserDataExportOptions;
};

export type ExportUserDataResult = {
	// The pretty-printed JSON archive the host writes to a file.
	archive: string;
	summary: UserDataExportSummary;
};

// One mod's row in an archive manifest: identity plus which facets it carries.
export type UserDataManifestModEntry = {
	modId: string;
	isLocal: boolean;
	version: string;
	name: string | null;
	// false marks a reference-only repository mod (its import needs the network).
	hasSource: boolean;
	hasSettings: boolean;
	hasConfig: boolean;
};

// What inspectUserData projects: the metadata and per-mod availability an import
// UI reads to build a selection over a specific archive.
export type UserDataManifest = {
	exportedAt: string | null;
	hasAppSettings: boolean;
	mods: UserDataManifestModEntry[];
};

export type UserDataImportModOutcome = {
	modId: string;
	status: 'installed' | 'skipped' | 'failed';
	// The failure/skip reason; absent for an installed mod.
	message?: string;
};

// The import summary: one outcome per processed mod, plus the app-settings intents
// when app settings were applied (absent otherwise).
export type UserDataImportSummary = {
	mods: UserDataImportModOutcome[];
	appSettings?: AppSettingsIntents;
};

export type ImportUserDataInput = {
	archive: string;
	selection: UserDataSelection;
	options: UserDataImportOptions;
};

export type ImportUserDataResult = {
	summary: UserDataImportSummary;
};

// A per-mod progress marker importUserData emits: a status marker (the installing
// start and a terminal installed/skipped/failed) or a forwarded install sub-event
// (compileTarget set - a local compile's target), both carrying the mod
// { modId, index, total } position. item is the union discriminant, always mod here.
export type ImportUserDataModProgress = {
	item: 'mod';
	modId: string;
	index: number;
	total: number;
	status?: 'installing' | 'installed' | 'skipped' | 'failed';
	message?: string;
	compileTarget?: string;
};

// The app-settings step marker: applying as the import starts writing the archive's
// global app settings, applied once done. Emitted once, before the mods, carrying no
// mod position.
export type ImportUserDataAppSettingsProgress = {
	item: 'appSettings';
	status: 'applying' | 'applied';
};

// A progress event importUserData emits: a per-mod marker or the app-settings step
// marker, discriminated by item.
export type ImportUserDataProgress =
	| ImportUserDataModProgress
	| ImportUserDataAppSettingsProgress;

////////////////////////////////////////////////////////////
// Hotkey capture (captureHotkey).

// The modifier keys held during a capture, each folded from its left and right
// variants; what captureHotkey emits as progress on every change of the set.
export type HotkeyCaptureModifiers = {
	ctrl: boolean;
	alt: boolean;
	shift: boolean;
	win: boolean;
};

// Why a capture completed with no chord: canceled through the handle (or
// superseded by a newer capture), the foreground window changed, or the
// capture's 15-second deadline passed.
export type HotkeyCaptureCanceled = 'canceled' | 'focusLost' | 'timeout';

// The completion of captureHotkey: the chord in the form a hotkey setting stores
// (ctrl+alt+84: the modifiers held, lowercase in that order, then the virtual-key
// code in decimal), or null with the reason there is none.
export type CaptureHotkeyResult = {
	hotkey: string | null;
	canceled?: HotkeyCaptureCanceled;
};

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
	// The Windows account the session acts for, which a user configuration of a
	// mod belongs to: the canonical SID string, and DOMAIN\name (the SID again
	// when the name cannot be looked up).
	user: { sid: string; name: string };
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

// One mod, on the terms listInstalledMods lists them on: `language` for the
// metadata parse, `checkForUpdates` gating the cached repository version.
export type GetInstalledModDetailsParams = {
	modId: string;
	language: string;
	checkForUpdates: boolean;
};

// An installed mod as the core reports one. It carries the TERMS of the update
// answer and never the answer: whether one is on offer is
//   latestVersion !== null && latestVersion !== metadata?.version
//     && !suppressesUpdateOffer(config.updatesDisabledForVersion, latestVersion)
// which the webview contract's `resolveUpdateOffer` applies for the front-end,
// and `InstalledModListEntry::is_update_available` for the core's own callers.
// A holder of the entry reaches the answer; a holder of the ANSWER would be
// holding one reached before the terms beside it moved, and those move one
// message at a time (a config write turns an offer down, an install takes a
// version).
export type InstalledModListEntry = {
	metadata: ModMetadata | null;
	config: ModConfig | null;
	// The version the repository holds, as the profile last cached it, or null
	// where the core knows of none (updates not asked for, a local mod, nothing
	// cached). Not suppression-aware, so a refused offer still names what was
	// refused - which is what tells that state from a mod that is up to date.
	latestVersion: string | null;
	userRating: number;
};

export type ListInstalledModsResult = {
	mods: Record<string, InstalledModListEntry>;
	// Mods whose source failed to parse, with the failure rendered as
	// `String(error)`. Surfacing is front-end policy (GUI notification,
	// CLI stderr warning).
	loadErrors: { modId: string; error: string }[];
};

// Same field set as the operations-layer InstallModInput minus modsFolderUrl:
// the repository folder URL for precompiled downloads is core-internal
// knowledge (the front-ends no longer know repository URLs).
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

export type TrayAction = 'restartBg';

////////////////////////////////////////////////////////////
// Async operations and events.

// Handle for a long-running command. cancel() is synchronous and cooperative
// (mirrors WhCoreCancel): it signals the operation, which then terminates with
// a cancellation error (CompilerKilled for compiles, an abort for update
// downloads); cancel of a finished operation is a harmless no-op.
export interface AsyncOperation<T> {
	readonly result: Promise<T>;
	cancel(): boolean;
}

// Events of startUpdate; reproduces exactly what the front-ends consume
// (download percentage and the "installing" transition).
export interface UpdateEvents {
	onProgress: (data: { progress: number }) => void;
	onInstalling: () => void;
}

// Events of importUserData: the per-mod progress stream (markers + forwarded
// compile sub-events), consumed by the front-end's import progress view.
export interface ImportEvents {
	onProgress: (data: ImportUserDataProgress) => void;
}

// Events of captureHotkey: the held modifiers, on every change of the set
// before the chord completes.
export interface HotkeyCaptureEvents {
	onModifiers: (modifiers: HotkeyCaptureModifiers) => void;
}

////////////////////////////////////////////////////////////
// The core interface.

// The single surface through which the front-end accesses the shared core. One
// method per command of the windhawk-core command inventory. Every method is
// async (Promise or AsyncOperation): the DLL-backed client cannot offer
// synchronous calls.
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
	// profile-derived latestVersion/userRating, and (optionally) the profile
	// reconciliation both front-ends perform.
	listInstalledMods(params: ListInstalledModsParams): Promise<ListInstalledModsResult>;

	// The listing's entry for ONE mod, without the scan: for a caller that has
	// just changed a single mod and needs the profile-held fields the change is
	// about (the install and recompile replies). The core builds it through the
	// same function the listing builds every entry with, so the two cannot
	// answer differently. No syncProfile twin - the reconciliation is a pass
	// over every installed mod, which is the listing's business.
	getInstalledModDetails(
		params: GetInstalledModDetailsParams
	): Promise<InstalledModListEntry>;

	// Stored source for a mod id. Rejects with MOD_NOT_INSTALLED when the
	// source file is missing.
	getModSource(modId: string): Promise<string>;

	// Whether a mod occupies a storage id (source file or config entry).
	doesModExist(modId: string): Promise<boolean>;

	getModConfig(modId: string): Promise<ModConfig | null>;

	// Patch semantics: absent fields are preserved.
	updateModConfig(modId: string, patch: ModConfigPatch): Promise<void>;

	// Switch the user's own configuration of a mod on (active) or off, back to
	// the global one. The first switch on copies the global configuration into
	// a new user configuration; switching off keeps it for a later switch on.
	// Resolves with the mod's whole config in the new scope.
	setModUserScope(modId: string, active: boolean): Promise<ModConfig>;

	// Remove the user's own configuration of a mod (a no-op for a user with
	// none). Resolves with the mod's whole config, now the global one.
	deleteModUserConfig(modId: string): Promise<ModConfig>;

	getModSettings(modId: string): Promise<ModSettings>;

	setModSettings(modId: string, settings: ModSettings): Promise<void>;

	// The options a mod wrote at runtime for its $dynamicSelect settings, read
	// out of its local-storage tree (a different tree from the settings, so a
	// settings write never clears them). An empty map for a mod that wrote
	// none; does not read the mod's source, so whether a path names a
	// $dynamicSelect item is the caller's lookup.
	getModDynamicSelectOptions(modId: string): Promise<ModDynamicSelectOptions>;

	// Scoped single-field write used by the editor sidebar's logging toggle.
	setModLoggingEnabled(modId: string, enable: boolean): Promise<void>;

	// User-profile write (rating of 0 clears the entry).
	setModRating(modId: string, rating: number): Promise<void>;

	// --- Review votes ---

	// Record an upvote on a review of a repository mod as a profile write and
	// answer the mod's whole list of votes after it, in cast order. A repeat of
	// an id already recorded writes nothing and answers the same list. The mod
	// need not be installed. Rejects a reviewId below 1 and a local@ id as
	// INVALID_REQUEST (the server's ids start at 1; a local mod has no reviews).
	voteModReview(modId: string, reviewId: number): Promise<ModReviewVote[]>;

	// Take a vote back as a profile write and answer the mod's whole list of
	// votes after it. An id that is not recorded writes nothing and answers the
	// list as it is. The core does not check when the vote was cast; how long a
	// vote stays retractable is the front-end's rule. The same refusals as
	// voteModReview.
	retractModReviewVote(modId: string, reviewId: number): Promise<ModReviewVote[]>;

	// The votes recorded for a mod, in cast order; empty for a mod the profile
	// has no entry for. Rejects a local@ id like voteModReview.
	getModReviewVotes(modId: string): Promise<ModReviewVote[]>;

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
	// profile. Returns whether the profile changed; the tray learns about
	// new versions from its own watcher on the profile file.
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

	// --- Host queries ---

	// The installed font families for a fontFamily setting's completion, as a
	// font picker lists them: deduplicated, sorted case-insensitively, without
	// the @-prefixed vertical variants (an additive command). Enumerated
	// through GDI, so the list matches the native host's.
	listFontFamilies(): Promise<string[]>;

	// Record the next keyboard chord the user presses, for a hotkey setting's
	// badge. The core installs a low-level keyboard hook for the capture's
	// duration, so the chord is seen ahead of Windows and nothing acts on it.
	// Resolves when the capture ends, with the chord or the reason there is
	// none; rejects when the capture could not start (HOTKEY_CAPTURE_UNAVAILABLE,
	// or an older DLL's INVALID_REQUEST). Cancel via the returned handle; the
	// core runs one capture per session and ends the running one when another
	// starts.
	captureHotkey(events: HotkeyCaptureEvents): AsyncOperation<CaptureHotkeyResult>;

	// --- User-data export/import ---

	// Aggregate the selected user data into an archive (the pretty-printed JSON
	// string the host writes to a file) plus a best-effort per-mod warning
	// summary. Local reads only (no network, no compile), so it is a plain
	// Promise.
	exportUserData(input: ExportUserDataInput): Promise<ExportUserDataResult>;

	// Validate an archive string and project its manifest (the parts available to
	// import). Pure over the archive; no session I/O.
	inspectUserData(archive: string): Promise<UserDataManifest>;

	// Install the archive's mods and restore their config/settings and the app
	// settings, per the selection. Async (it compiles): per-mod progress events,
	// then a per-mod outcome summary. Cancel via the returned handle.
	importUserData(input: ImportUserDataInput, events: ImportEvents): AsyncOperation<ImportUserDataResult>;
}

////////////////////////////////////////////////////////////
// Runtime mirrors of the core's closed sets.
//
// The command inventory and the contract's closed string domains are frozen
// sets owned by windhawk-core. A set that changes in the core repo has to
// change in its mirror below too, or the DLL's deserializer rejects the stale
// value at runtime.

// The command inventory as a runtime list, mirroring the WindhawkCore
// interface.
export type CommandName = keyof WindhawkCore;

export const ALL_COMMANDS: readonly CommandName[] = [
	'getCoreInfo',
	'parseModSource',
	'appendToModIdAndName',
	'listInstalledMods',
	'getInstalledModDetails',
	'getModSource',
	'doesModExist',
	'getModConfig',
	'updateModConfig',
	'getModSettings',
	'setModSettings',
	'setModLoggingEnabled',
	'setModRating',
	'installMod',
	'compileInstalledMod',
	'setModEnabled',
	'removeMod',
	'applyAppSettings',
	'previewAppSettingsEffects',
	'syncCatalogToProfile',
	'getAppSettings',
	'fetchCatalog',
	'fetchRepoModSource',
	'fetchModVersions',
	'getAppUpdateStatus',
	'getProfileWatchInfo',
	'notifyTray',
	'startUpdate',
	'getCompileFlags',
	'exportUserData',
	'inspectUserData',
	'importUserData',
	'getModDynamicSelectOptions',
	'listFontFamilies',
	'captureHotkey',
	'voteModReview',
	'retractModReviewVote',
	'getModReviewVotes',
	'setModUserScope',
	'deleteModUserConfig',
];

// Build a runtime list that is exactly the domain of a closed string union.
// Both directions are checked when this file compiles: a value outside T fails
// the `readonly T[]` constraint, and a member of T left out of the list makes
// the parameter type `never`, which no argument satisfies. That ties each list
// to its union.
function valueDomain<T extends string>() {
	return <L extends readonly T[]>(values: L & ([T] extends [L[number]] ? unknown : never)): L =>
		values;
}

// The closed string domains, keyed by the name of the core enum each mirrors.
// A domain the core owns but the front-end does not model as a union (the error
// codes: WindhawkError.code is an open string) is absent here.
export const CORE_VALUE_DOMAINS = {
	TrayAction: valueDomain<TrayAction>()(['restartBg'] as const),
	UserScope: valueDomain<UserScope>()(['none', 'inactive', 'active'] as const),
	ModScopeKeyword: valueDomain<Extract<UserDataModScope, string>>()([
		'all',
		'all-except-local',
		'none',
	] as const),
	ConflictPolicy: valueDomain<UserDataImportOptions['onConflict']>()([
		'overwrite',
		'skip',
	] as const),
	ImportModStatus: valueDomain<UserDataImportModOutcome['status']>()([
		'installed',
		'skipped',
		'failed',
	] as const),
	HotkeyCaptureCanceled: valueDomain<HotkeyCaptureCanceled>()([
		'canceled',
		'focusLost',
		'timeout',
	] as const),
	ImportProgressStatus: valueDomain<NonNullable<ImportUserDataModProgress['status']>>()([
		'installing',
		'installed',
		'skipped',
		'failed',
	] as const),
	ImportProgressItem: valueDomain<ImportUserDataProgress['item']>()(['mod', 'appSettings'] as const),
	ImportAppSettingsStatus: valueDomain<ImportUserDataAppSettingsProgress['status']>()([
		'applying',
		'applied',
	] as const),
};

// The scalar constants restated above, keyed by the name of the core constant
// each mirrors. A constant the core owns but the extension does not restate
// (the theme default) is absent here.
export const CORE_CONSTANTS = {
	CONTRACT_VERSION,
	MAX_ARCHIVE_BYTES,
};
