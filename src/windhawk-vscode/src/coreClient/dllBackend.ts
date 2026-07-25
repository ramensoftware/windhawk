import * as fs from 'fs';
import * as path from 'path';
import { Logger } from './logger';
import {
	debugCoreBridgePath,
	debugCoreDllPath,
	debugIgnoreCertErrors,
	debugInstallerRegKeyString,
	debugModsUrlRoot,
	debugSchtasksPath,
	debugUpdateInstallerUrl,
} from './debugOverrides';
import {
	AppSettings,
	AppSettingsIntents,
	AppUpdateStatus,
	AsyncOperation,
	Catalog,
	CatalogForProfileSync,
	CompileInstalledModInput,
	CompileInstalledModResult,
	CompilerError,
	CompilerKilled,
	CONTRACT_VERSION,
	CoreInfo,
	CoreInstallModInput,
	ExportUserDataInput,
	ExportUserDataResult,
	ImportEvents,
	ImportUserDataInput,
	ImportUserDataProgress,
	ImportUserDataResult,
	InstallModResult,
	ListInstalledModsParams,
	ListInstalledModsResult,
	ModConfig,
	ModSettings,
	ModVersionInfo,
	ParsedModSource,
	ProfileWatchInfo,
	TrayAction,
	UpdateEvents,
	UserDataManifest,
	WindhawkCore,
	WindhawkError,
} from './contract';

// The windhawk-core.dll backend: loads the napi-rs bridge (.node), which
// loads windhawk-core.dll, and serves the whole contract over the JSON command
// protocol. It is the only backend - there is no in-process fallback, so a
// missing or incompatible DLL is fatal (createWindhawkCore surfaces it).
//
// Artifact discovery (development overrides are gated like the other
// WINDHAWK_DEBUG_* vars, see ./debugOverrides.ts):
//   WINDHAWK_DEBUG_CORE_BRIDGE_PATH - the bridge .node binary. Default:
//     prebuilds/windhawk-core/win32-<arch>/windhawk-core-bridge.node next
//     to the bundle (vendored at the pinned windhawk-core version).
//   WINDHAWK_DEBUG_CORE_DLL_PATH - windhawk-core.dll. Defaults: next to
//     the bridge prebuild, then <appRoot>\windhawk-core.dll (the proposed
//     production layout).

// The bridge surface (the windhawk-core repository's bridge crate). The
// bridge is dumb plumbing: no command names, no DTO knowledge, no error
// mapping.
type BridgeCoreSession = {
	invoke(requestJson: string): Promise<string>;
	invokeAsync(requestJson: string): number;
	cancel(opId: number): boolean;
	destroy(): void;
};

type BridgeCoreLibrary = {
	getAbiVersion(): number;
	getInfoJson(): string;
	createSession(
		configJson: string,
		onLog: (level: number, message: string) => void,
		onEvent: (opId: number, eventJson: string) => void,
	): BridgeCoreSession;
};

export type BridgeModule = {
	loadCore(dllPath: string): BridgeCoreLibrary;
};

// One event of an async operation: a stream of command-specific progress
// (startUpdate's { progress } download percentage, the compile ops' per-target
// { compileTarget }) plus the update flow's one-shot installing transition,
// terminated by exactly one completed or failed. The bridge delivers these as
// eventJson on the JS thread, in order, via the session's onEvent callback.
type OperationEvent =
	| {
			type: 'progress';
			payload: { progress: number } | { compileTarget: string } | ImportUserDataProgress;
	  }
	| { type: 'installing' }
	| { type: 'completed'; result: unknown }
	| { type: 'failed'; error?: { code?: string; message?: string; details?: unknown } };

// Failure envelope of a DLL-served command. Extends WindhawkError (NOT plain
// Error) so its stable wire `code` participates in the front-ends' error
// handling - in particular the CLI's exit-code mapping (cli/output.ts), which
// only consults `.code` when the error is `instanceof WindhawkError`. Without
// this every DLL failure that
// toCompileError does not rebuild into a typed CompilerError/CompilerKilled
// would collapse to the GENERIC exit code 1 instead of its MOD_NOT_INSTALLED
// (4) / MOD_NOT_IN_REPO (5) / REPO_UNREACHABLE (6) / ... exit code. (The three
// wire spellings the CLI map keys differ on - APP_ROOT_INVALID,
// COMPILER_FAILED, CANCELED - are aliased there.) WindhawkError sets `.name` to
// the subclass name (CoreDllError) and `.code` from its first argument, so both
// are preserved.
export class CoreDllError extends WindhawkError {
	public readonly details: unknown;

	public constructor(code: string, message: string, details?: unknown) {
		super(code, message);
		this.details = details;
	}
}

// Build a CoreDllError from an error envelope's error object (a failed event
// or a response envelope).
function toCoreDllError(error?: { code?: string; message?: string; details?: unknown }): CoreDllError {
	const e = error ?? {};
	return new CoreDllError(e.code ?? 'INTERNAL', e.message ?? 'unknown windhawk-core error', e.details);
}

// invokeAsync throws on a synchronous start failure (malformed request,
// unknown command, UPDATE_IN_PROGRESS): the bridge surfaces the error
// envelope JSON as the thrown Error's message. Recover the structured error
// where possible; otherwise wrap the raw message (e.g. "session has been
// destroyed") as INTERNAL.
function parseStartError(e: unknown): CoreDllError {
	const message = e instanceof Error ? e.message : String(e);
	try {
		const parsed = JSON.parse(message) as { error?: { code?: string; message?: string; details?: unknown } };
		if (parsed && typeof parsed === 'object' && parsed.error) {
			return toCoreDllError(parsed.error);
		}
	} catch {
		// Not an error envelope; fall through to a generic wrap.
	}
	return new CoreDllError('INTERNAL', message);
}

// Map a compile-bearing failure (compileInstalledMod, and installMod's
// compile-locally path) back to the typed errors the front-ends branch on with
// `instanceof` (the contract exports the very classes the front-ends branch on,
// so the boundary is transparent): COMPILER_FAILED rebuilds the
// CompilerError from the wire details (target/exitCode/stdout/stderr - its
// constructor reproduces the same message the core sent), and CANCELED becomes
// CompilerKilled. Any other failure (installMod's download REPO_UNREACHABLE /
// IO_FAILED / min-Windhawk-version INTERNAL) stays a CoreDllError.
function toCompileError(error?: { code?: string; message?: string; details?: unknown }): Error {
	if (error?.code === 'CANCELED') {
		return new CompilerKilled();
	}
	if (error?.code === 'COMPILER_FAILED') {
		const d = (error.details ?? {}) as {
			target?: string;
			exitCode?: number | null;
			stdout?: string;
			stderr?: string;
		};
		return new CompilerError(
			(d.target ?? '') as ConstructorParameters<typeof CompilerError>[0],
			d.exitCode ?? null,
			d.stdout ?? '',
			d.stderr ?? '',
		);
	}
	return toCoreDllError(error);
}

export type DllBackendOptions = {
	appRoot: string;
	windhawkVersion: string | null;
	// Optional repository User-Agent override. When omitted, the core builds
	// its GUI-style default ("Windhawk/<windhawkVersion>", plus " (portable)"
	// for portable installs) from the session config - which is exactly what
	// the extension wants, so createWindhawkCore passes nothing. A caller that
	// needs a different product identity forwards the full header value here.
	userAgent?: string;
	logger: Logger;
};

export type DllBackend = {
	// The full contract, served by the DLL. Typed as WindhawkCore (not
	// Partial) so adding an interface method fails compilation here until its
	// wrapper is added - a completeness guard over the DLL-served surface.
	commands: WindhawkCore;
};

// Load a native addon (.node) from an absolute path at runtime, bypassing
// the bundler. In the webpack bundle this is the production path:
// __non_webpack_require__ is the real Node require that webpack
// substitutes. In plain Node (tests, ts-node) that global is absent, so we
// fall back to a require obtained via eval - a bare `require(variablePath)`
// would make webpack build a directory "context" for the call and warn,
// and eval keeps it from statically analyzing this branch (which never
// runs in the bundle).
declare const __non_webpack_require__: NodeRequire | undefined;
function nativeRequire(modulePath: string): unknown {
	if (typeof __non_webpack_require__ !== 'undefined') {
		return __non_webpack_require__(modulePath);
	}
	const nodeRequire = eval('require') as NodeRequire;
	return nodeRequire(modulePath);
}

function prebuildDir(): string {
	// __dirname is dist/ in the bundle; prebuilds/ sits next to it at the
	// package root, like the native-reg prebuilds.
	return path.join(__dirname, '..', 'prebuilds', 'windhawk-core', `win32-${process.arch}`);
}

function resolveBridgePath(): string {
	return debugCoreBridgePath() ?? path.join(prebuildDir(), 'windhawk-core-bridge.node');
}

function resolveDllPath(appRoot: string): string {
	const override = debugCoreDllPath();
	if (override) {
		return override;
	}
	const candidates = [
		path.join(prebuildDir(), 'windhawk-core.dll'),
		path.join(appRoot, 'windhawk-core.dll'),
	];
	return candidates.find(p => fs.existsSync(p)) ?? candidates[0];
}

function loadBridgeFromDisk(): BridgeModule {
	const bridgePath = resolveBridgePath();
	if (!fs.existsSync(bridgePath)) {
		throw new Error(`bridge not found: ${bridgePath}`);
	}
	return nativeRequire(bridgePath) as BridgeModule;
}

// Load the bridge and the DLL and create the core session. Throws when
// either binary is missing or incompatible; the failure is fatal (there is no
// in-process fallback). `bridgeOverride` injects a fake bridge for tests (the
// production path loads the prebuilt .node from disk).
export function createDllBackend(options: DllBackendOptions, bridgeOverride?: BridgeModule): DllBackend {
	const { appRoot, windhawkVersion, userAgent, logger } = options;

	const bridge = bridgeOverride ?? loadBridgeFromDisk();

	// The bridge validates WhCoreGetAbiVersion itself; the contract version
	// is validated here, where the contract lives.
	const library = bridge.loadCore(resolveDllPath(appRoot));
	const info = JSON.parse(library.getInfoJson()) as { contractVersion: string };
	if (info.contractVersion !== CONTRACT_VERSION) {
		// Version skew must be loud: this is a packaging error, not a normal
		// missing-artifact development state.
		const message =
			`windhawk-core contract version mismatch: DLL has ${info.contractVersion}, ` +
			`client expects ${CONTRACT_VERSION}`;
		logger.error(message);
		throw new Error(message);
	}

	// In-flight async operations, keyed by the operation id the bridge
	// reports. JS is single-threaded and the bridge queues onEvent onto the
	// event loop, so a handler registered synchronously after invokeAsync
	// returns is in place before any of its operation's events can dispatch.
	const opHandlers = new Map<number, (event: OperationEvent) => void>();

	const session = library.createSession(
		JSON.stringify({
			appRootPath: appRoot,
			windhawkVersion,
			userAgent,
			debugOverrides: {
				modsUrlRoot: debugModsUrlRoot() ?? null,
				updateUrl: debugUpdateInstallerUrl() ?? null,
				installerRegKey: debugInstallerRegKeyString() ?? null,
				schtasksPath: debugSchtasksPath() ?? null,
				ignoreCertErrors: debugIgnoreCertErrors(),
			},
		}),
		(level, message) => {
			const log = level === 0 ? logger.error : level === 1 ? logger.warn : logger.info;
			log.call(logger, `windhawk-core: ${message}`);
		},
		(opId, eventJson) => {
			// Deliver the event to its operation's handler (the dispatcher
			// deferred until the first async command landed).
			// An unknown id is a harmless no-op (a terminated operation).
			opHandlers.get(opId)?.(JSON.parse(eventJson) as OperationEvent);
		},
	);
	// The session lives for the process: the WindhawkCore contract has no
	// dispose, and the bridge unrefs its callbacks (a leaked session cannot
	// keep the process alive) and tears the session down on GC/exit.

	async function invoke<T>(command: string, params: unknown): Promise<T> {
		const response = JSON.parse(await session.invoke(JSON.stringify({ command, params }))) as
			| { ok: true; result: T }
			| { ok: false; error?: { code?: string; message?: string; details?: unknown } };
		if (response.ok) {
			return response.result;
		}
		throw toCoreDllError(response.error);
	}

	// Start an async command and register its event handler. invokeAsync
	// returns the operation id (or throws the start-failure envelope); the
	// handler is keyed on that id. Returns the id so the caller can cancel.
	function startAsync(
		command: string,
		params: unknown,
		handler: (event: OperationEvent) => void,
	): number {
		const opId = session.invokeAsync(JSON.stringify({ command, params }));
		opHandlers.set(opId, handler);
		return opId;
	}

	// An async command that the contract exposes as a plain Promise (the
	// repository fetches, which emit no progress): resolve on completed,
	// reject on failed, and deregister on either.
	function invokeAsyncToPromise<T>(command: string, params: unknown): Promise<T> {
		return new Promise<T>((resolve, reject) => {
			let opId = -1;
			const handler = (event: OperationEvent) => {
				if (event.type === 'completed') {
					opHandlers.delete(opId);
					resolve(event.result as T);
				} else if (event.type === 'failed') {
					opHandlers.delete(opId);
					reject(toCoreDllError(event.error));
				}
				// progress/installing are not emitted by these commands.
			};
			try {
				opId = startAsync(command, params, handler);
			} catch (e) {
				reject(parseStartError(e));
			}
		});
	}

	// startUpdate returns an AsyncOperation synchronously: progress and
	// installing events drive the caller's
	// UpdateEvents, result resolves on completed / rejects on failed
	// (including a synchronous UPDATE_IN_PROGRESS, which rejects result rather
	// than throwing), and cancel() signals the operation.
	function startUpdate(events: UpdateEvents): AsyncOperation<void> {
		let opId = -1;
		const result = new Promise<void>((resolve, reject) => {
			const handler = (event: OperationEvent) => {
				switch (event.type) {
					case 'progress':
						// startUpdate's progress payload is always { progress }; the
						// { compileTarget } shape only rides the compile ops.
						if ('progress' in event.payload) {
							events.onProgress(event.payload);
						}
						break;
					case 'installing':
						events.onInstalling();
						break;
					case 'completed':
						opHandlers.delete(opId);
						resolve();
						break;
					case 'failed':
						opHandlers.delete(opId);
						reject(toCoreDllError(event.error));
						break;
				}
			};
			try {
				opId = startAsync('startUpdate', {}, handler);
			} catch (e) {
				reject(parseStartError(e));
			}
		});
		return {
			result,
			cancel: () => (opId >= 0 ? session.cancel(opId) : false),
		};
	}

	// The compile-bearing async operations (compileInstalledMod and installMod)
	// return an AsyncOperation synchronously. They emit a per-target `progress`
	// event ({ compileTarget }) as each architecture compiles - this backend
	// ignores it and drives only on the terminal completed/failed - and map
	// failures to the typed compiler errors so the front-ends' CompilerError/
	// CompilerKilled handling is backend-agnostic. A synchronous start failure
	// (e.g. INVALID_REQUEST) is not a compiler failure, so it stays a CoreDllError.
	function startCompileLikeOperation<T>(command: string, params: unknown): AsyncOperation<T> {
		let opId = -1;
		const result = new Promise<T>((resolve, reject) => {
			const handler = (event: OperationEvent) => {
				if (event.type === 'completed') {
					opHandlers.delete(opId);
					resolve(event.result as T);
				} else if (event.type === 'failed') {
					opHandlers.delete(opId);
					reject(toCompileError(event.error));
				}
			};
			try {
				opId = startAsync(command, params, handler);
			} catch (e) {
				reject(parseStartError(e));
			}
		});
		return {
			result,
			cancel: () => (opId >= 0 ? session.cancel(opId) : false),
		};
	}

	// importUserData returns an AsyncOperation synchronously, like startUpdate but
	// for the import stream: per-mod progress events (markers + forwarded compile
	// sub-events, each carrying the mod position) drive the caller's ImportEvents;
	// result resolves on completed with the summary, or rejects on failed (a
	// validation error, a missing offline source, or a cancellation) / a synchronous
	// start failure. A per-mod compile failure is a progress result, not the terminal
	// failed, so the operation still completes (the summary reports it).
	function startImportUserData(
		input: ImportUserDataInput,
		events: ImportEvents,
	): AsyncOperation<ImportUserDataResult> {
		let opId = -1;
		const result = new Promise<ImportUserDataResult>((resolve, reject) => {
			const handler = (event: OperationEvent) => {
				switch (event.type) {
					case 'progress':
						// Every import progress payload carries the mod position; forward
						// it verbatim (the { progress } / bare { compileTarget } shapes ride
						// other operations, never this one).
						events.onProgress(event.payload as ImportUserDataProgress);
						break;
					case 'completed':
						opHandlers.delete(opId);
						resolve(event.result as ImportUserDataResult);
						break;
					case 'failed':
						opHandlers.delete(opId);
						reject(toCoreDllError(event.error));
						break;
				}
			};
			try {
				opId = startAsync('importUserData', input, handler);
			} catch (e) {
				reject(parseStartError(e));
			}
		});
		return {
			result,
			cancel: () => (opId >= 0 ? session.cancel(opId) : false),
		};
	}

	return {
		commands: {
			// --- Meta ---
			getCoreInfo: () => invoke<CoreInfo>('getCoreInfo', {}),

			// --- Pure helpers ---
			parseModSource: (source: string, language: string) =>
				invoke<ParsedModSource>('parseModSource', { source, language }),
			appendToModIdAndName: (source: string, appendToId?: string, appendToName?: string) =>
				invoke<string>('appendToModIdAndName', { source, appendToId, appendToName }),

			// --- Installed-mod queries and scoped writes ---
			listInstalledMods: (params: ListInstalledModsParams) =>
				invoke<ListInstalledModsResult>('listInstalledMods', params),
			getModSource: (modId: string) => invoke<string>('getModSource', { modId }),
			doesModExist: (modId: string) => invoke<boolean>('doesModExist', { modId }),
			getModConfig: (modId: string) => invoke<ModConfig | null>('getModConfig', { modId }),
			updateModConfig: (modId: string, patch: Partial<ModConfig>) =>
				invoke<void>('updateModConfig', { modId, patch }),
			getModSettings: (modId: string) => invoke<ModSettings>('getModSettings', { modId }),
			setModSettings: (modId: string, settings: ModSettings) =>
				invoke<void>('setModSettings', { modId, settings }),
			setModLoggingEnabled: (modId: string, enable: boolean) =>
				invoke<void>('setModLoggingEnabled', { modId, enable }),
			setModRating: (modId: string, rating: number) =>
				invoke<void>('setModRating', { modId, rating }),

			// --- Use-case operations (sync subset) ---
			setModEnabled: (modId: string, enable: boolean) =>
				invoke<void>('setModEnabled', { modId, enable }),
			removeMod: (modId: string) => invoke<void>('removeMod', { modId }),
			applyAppSettings: (patch: Partial<AppSettings>) =>
				invoke<AppSettingsIntents>('applyAppSettings', { patch }),
			previewAppSettingsEffects: (patch: Partial<AppSettings>) =>
				invoke<AppSettingsIntents>('previewAppSettingsEffects', { patch }),
			syncCatalogToProfile: (catalog: CatalogForProfileSync) =>
				invoke<{ profileUpdated: boolean }>('syncCatalogToProfile', { catalog }),

			// --- Use-case operations (async) ---
			// installMod and compileInstalledMod share the compile-bearing
			// AsyncOperation shape (per-target progress events, ignored; failures
			// map to CompilerError/CompilerKilled). installMod's input is the
			// contract's CoreInstallModInput verbatim - the core derives the
			// repository URL internally, so there is no modsFolderUrl to pass.
			installMod: (input: CoreInstallModInput) =>
				startCompileLikeOperation<InstallModResult>('installMod', input),
			compileInstalledMod: (input: CompileInstalledModInput) =>
				startCompileLikeOperation<CompileInstalledModResult>('compileInstalledMod', input),

			// --- App settings ---
			getAppSettings: () => invoke<AppSettings>('getAppSettings', {}),

			// --- Repository (network, async) ---
			fetchCatalog: (language: string) =>
				invokeAsyncToPromise<Catalog>('fetchCatalog', { language }),
			fetchRepoModSource: (modId: string, version?: string) =>
				invokeAsyncToPromise<string>('fetchRepoModSource', { modId, version }),
			fetchModVersions: (modId: string) =>
				invokeAsyncToPromise<ModVersionInfo[]>('fetchModVersions', { modId }),

			// --- User profile auxiliary ---
			getAppUpdateStatus: () => invoke<AppUpdateStatus>('getAppUpdateStatus', {}),
			getProfileWatchInfo: () => invoke<ProfileWatchInfo>('getProfileWatchInfo', {}),

			// --- Tray ---
			notifyTray: (action: TrayAction) => invoke<void>('notifyTray', { action }),

			// --- Update (async) ---
			startUpdate: (events: UpdateEvents) => startUpdate(events),

			// --- Editor support ---
			getCompileFlags: () => invoke<string[]>('getCompileFlags', {}),

			// --- User-data export/import ---
			exportUserData: (input: ExportUserDataInput) =>
				invoke<ExportUserDataResult>('exportUserData', input),
			inspectUserData: (archive: string) =>
				invoke<{ manifest: UserDataManifest }>('inspectUserData', { archive }).then(
					r => r.manifest,
				),
			importUserData: (input: ImportUserDataInput, events: ImportEvents) =>
				startImportUserData(input, events),
		},
	};
}
