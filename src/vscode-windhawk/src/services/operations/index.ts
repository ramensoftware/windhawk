import { Services } from '../index';
import { AppSettings, ModConfig, ModMetadata } from '../types';

// Shared use-case ("operations") layer between the low-level services and the
// two front-ends (the VSCode extension's IPC handlers and the windhawk-cli
// commands). Each operation single-sources a multi-step flow that previously
// lived in both front-ends and drifted.
//
// Design rules:
// - Operations perform data orchestration only and RETURN results plus
//   "intents". Front-end-specific side effects (tray spawns, OutputChannel
//   handling, editor workspace and drafts, webview replies, CLI output and
//   exit codes) stay in the front-ends, which decide policy from the
//   returned intents.
// - Per-front-end behavioral differences are explicit operation parameters,
//   never hidden branches.
// - Input validation that produces front-end-specific error types (e.g. the
//   CLI's UsageError vs the extension's plain Error messages) stays in the
//   front-ends; operations receive already-validated inputs.
// - The extension is the shipped GUI with no automated tests: every operation
//   body is a near-verbatim extraction of the extension's IPC handler code,
//   preserving the exact service-call sequence, arguments, and thrown errors.

export interface InstallModInput {
	// Storage id under which the mod is persisted: the bare repo id, or
	// local@<id> for locally-authored mods (file installs, editor mods).
	storageId: string;
	// Mod source code, CRLF-normalized by the caller.
	source: string;
	// Metadata already extracted from `source` and validated by the caller
	// (id present and reconciled against storageId).
	metadata: ModMetadata;
	// true/false sets the state explicitly; omitted preserves the existing
	// state (ModConfigCodec.serialize skips undefined fields), which on a
	// fresh install means the backend's default of enabled.
	disabled?: boolean;
	// Same omitted-means-preserve semantics as `disabled`.
	loggingEnabled?: boolean;
	// The compile-vs-download decision, made by the caller: the GUI uses its
	// cached alwaysCompileModsLocally setting, the CLI combines a fresh
	// settings read with its --file / --no-precompiled flags.
	compileLocally: boolean;
	// Repo folder URL for precompiled DLLs. Required when compileLocally is
	// false.
	modsFolderUrl?: string;
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
}

export interface InstallModResult {
	// The mod's config as read back from storage after the install.
	config: ModConfig;
	targetDllName: string;
}

export interface CompileInstalledModInput {
	// Storage id of the installed mod (bare id, or local@<id>).
	storageId: string;
	// The mod's stored source code, read by the caller.
	source: string;
	// Metadata already extracted from `source` and validated by the caller
	// (id present and reconciled against storageId modulo the local@ prefix).
	metadata: ModMetadata;
}

export interface CompileInstalledModResult {
	// The mod's config as read back from storage after the compile.
	config: ModConfig;
	targetDllName: string;
}

export interface AppSettingsIntents {
	requiresRestart: boolean;
	requiresNotify: boolean;
}

// Minimal structural slice of the repository catalog JSON that the profile
// sync needs. Both the extension's raw catalog response and the CLI's typed
// Catalog satisfy it.
export interface CatalogForProfileSync {
	app: {
		version?: string;
		versionBleedingEdge?: string;
	};
	mods: Record<string, {
		metadata: {
			version?: string;
		};
	}>;
}

export interface Operations {
	// Install or reinstall a mod from already-extracted source + metadata:
	// migrate mod settings, compile locally or download a precompiled DLL,
	// persist config and source, clean up DLLs of prior versions, and record
	// the version in the user profile. Also serves the editor compile flow
	// via pchFolder and renameFromStorageId. Source acquisition, metadata
	// validation, and compiler output routing are the caller's job;
	// CompilerError propagates unchanged.
	installMod(input: InstallModInput): Promise<InstallModResult>;

	// Recompile an already-installed mod from its stored source. Unlike
	// installMod, this writes no source, runs no settings migration, and
	// touches no user profile (none of those inputs changed); only the
	// library file name and the metadata-derived config fields get updated.
	// CompilerError propagates unchanged.
	compileInstalledMod(input: CompileInstalledModInput): Promise<CompileInstalledModResult>;

	// Enable or disable an installed mod, and mirror the new state into the
	// user profile for non-local mods so GUI and CLI reads stay consistent.
	// Callers are responsible for any existence or already-in-state checks
	// they want to make beforehand; this always writes.
	setModEnabled(modId: string, enable: boolean): void;

	// Uninstall a mod: delete its config, source, and DLLs, and drop its
	// profile entry for non-local mods. Data only: the extension's editor
	// draft cleanup for local@ mods is the caller's job, as are existence
	// checks and confirmation gates.
	removeMod(modId: string): void;

	// Apply an app-settings patch and return what the change demands of the
	// tray program. The operation always writes; the caller decides tray policy
	// from the returned intents (the extension spawns the tray, the CLI does
	// not). The CLI's --confirm-app-restart refusal gate must be enforced by
	// the caller BEFORE calling this; the GUI has no such gate.
	applyAppSettings(patch: Partial<AppSettings>): AppSettingsIntents;

	// Record the catalog's latest app and per-mod versions in the user
	// profile. Returns whether the profile changed (i.e. new updates were
	// found); the extension uses that to post the tray's "new updates found"
	// notification, the CLI does not.
	syncCatalogToProfile(catalog: CatalogForProfileSync): { profileUpdated: boolean };
}

export function createOperations(services: Services): Operations {
	return {
		async installMod(input) {
			const { storageId, source, metadata } = input;

			const initialSettings = services.modSource.extractInitialSettingsForEngine(source);

			let previousInitialSettings: Record<string, string | number> | undefined;
			try {
				const prev = services.modSource.extractInitialSettingsForEngine(
					services.modSource.getSource(storageId)
				);
				if (prev) {
					previousInitialSettings = prev;
				}
			} catch (e) {
				if (e.code !== 'ENOENT') {
					console.error('Failed to extract previous initial settings for engine:', e);
				}
			}

			let targetDllName: string;
			if (input.compileLocally) {
				const result = await services.compiler.compileMod(
					storageId,
					metadata.version || '',
					metadata.include || [],
					source,
					metadata.architecture || [],
					metadata.compilerOptions,
					input.pchFolder
				);
				targetDllName = result.targetDllName;
			} else {
				if (input.modsFolderUrl === undefined) {
					throw new Error('modsFolderUrl is required when compileLocally is false');
				}
				const result = await services.modFiles.downloadPrecompiledMod(
					storageId,
					metadata.version || '',
					metadata.architecture || [],
					input.modsFolderUrl
				);
				targetDllName = result.targetDllName;
			}

			if (input.renameFromStorageId !== undefined) {
				services.modConfig.changeModId(input.renameFromStorageId, storageId);
			}

			services.modConfig.setModConfig(storageId, {
				libraryFileName: targetDllName,
				disabled: input.disabled,
				loggingEnabled: input.loggingEnabled,
				// debugLoggingEnabled: false,
				include: metadata.include || [],
				exclude: metadata.exclude || [],
				// includeCustom: [],
				// excludeCustom: [],
				// includeExcludeCustomOnly: false,
				// patternsMatchCriticalSystemProcesses: false,
				architecture: metadata.architecture || [],
				version: metadata.version || ''
			}, {
				initialSettings: initialSettings || {},
				previousInitialSettings
			});

			services.modSource.setSource(storageId, source);

			if (input.renameFromStorageId !== undefined) {
				services.modSource.deleteSource(input.renameFromStorageId);
			}

			services.modFiles.deleteOldModFiles(storageId, metadata.architecture || [], targetDllName);

			if (input.trackInProfile) {
				const userProfile = services.userProfile.read();
				userProfile.setModVersion(storageId, metadata.version || '');
				userProfile.write();
			}

			const config = services.modConfig.getModConfig(storageId);
			if (!config) {
				throw new Error('Failed to query installed mod details');
			}

			return { config, targetDllName };
		},

		async compileInstalledMod(input) {
			const { storageId, source, metadata } = input;

			const { targetDllName } = await services.compiler.compileMod(
				storageId,
				metadata.version || '',
				metadata.include || [],
				source,
				metadata.architecture || [],
				metadata.compilerOptions
			);

			services.modConfig.setModConfig(storageId, {
				libraryFileName: targetDllName,
				// disabled: false,
				// loggingEnabled: false,
				// debugLoggingEnabled: false,
				include: metadata.include || [],
				exclude: metadata.exclude || [],
				// includeCustom: [],
				// excludeCustom: [],
				// includeExcludeCustomOnly: false,
				// patternsMatchCriticalSystemProcesses: false,
				architecture: metadata.architecture || [],
				version: metadata.version || ''
			});

			services.modFiles.deleteOldModFiles(storageId, metadata.architecture || [], targetDllName);

			const config = services.modConfig.getModConfig(storageId);
			if (!config) {
				throw new Error('Failed to query compiled mod details');
			}

			return { config, targetDllName };
		},

		setModEnabled(modId, enable) {
			services.modConfig.enableMod(modId, enable);

			if (!modId.startsWith('local@')) {
				const userProfile = services.userProfile.read();
				userProfile.setModDisabled(modId, !enable);
				userProfile.write();
			}
		},

		removeMod(modId) {
			services.modConfig.deleteMod(modId);
			services.modSource.deleteSource(modId);

			services.modFiles.deleteModFiles(modId);

			if (!modId.startsWith('local@')) {
				const userProfile = services.userProfile.read();
				userProfile.deleteMod(modId);
				userProfile.write();
			}
		},

		applyAppSettings(patch) {
			services.appSettings.updateAppSettings(patch);

			// Both predicates are pure functions of the patch, so computing
			// them after the write keeps the extension's write-then-check
			// order. Callers that need the restart intent before the write
			// (the CLI's confirmation gate) call shouldRestartApp directly.
			return {
				requiresRestart: services.appSettings.shouldRestartApp(patch),
				requiresNotify: services.appSettings.shouldNotifyTrayProgram(patch),
			};
		},

		syncCatalogToProfile(catalog) {
			const userProfile = services.userProfile.read();

			const appLatestVersion = catalog.app.version;
			const appLatestVersionBleedingEdge = catalog.app.versionBleedingEdge;

			const modLatestVersion: Record<string, string> = {};
			for (const [modId, value] of Object.entries(catalog.mods)) {
				const { version } = value.metadata;
				if (version) {
					modLatestVersion[modId] = version;
				}
			}

			if (!userProfile.updateLatestVersions(appLatestVersion, appLatestVersionBleedingEdge, modLatestVersion)) {
				return { profileUpdated: false };
			}

			// Write as an external update so the extension's user-profile file
			// watcher forwards the new data to the UI. Inert for the CLI: the
			// flag only affects in-process mtime bookkeeping.
			const asExternalUpdate = true;
			userProfile.write(asExternalUpdate);

			return { profileUpdated: true };
		},
	};
}
