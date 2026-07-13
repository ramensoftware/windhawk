import * as semver from 'semver';
import type { Services } from '../services';
import { createOperations, Operations } from '../services/operations';
import {
	AsyncOperation,
	CONTRACT_VERSION,
	CoreFsPaths,
	CoreInstallModInput,
	InstallModResult,
	InstalledModListEntry,
	ListInstalledModsParams,
	ListInstalledModsResult,
	WindhawkCore,
} from './contract';
import { parseModSourceWith } from './parseModSource';

// The in-process core backend: implements the WindhawkCore contract by
// delegating to the existing TypeScript services. Composite commands
// (listInstalledMods) carry the exact service-call sequences extracted from
// the extension's IPC handlers, like the operations layer before them; the
// characterization tests in src/test/ pin those sequences. Deleted when
// the native core serves every command.
//
// Construction takes an already-built Services bundle (instead of building
// one) so tests can inject recording fakes without loading the native
// modules the real storage backends require.

export type CoreEnvironment = {
	portable: boolean;
	arm64Enabled: boolean;
	// Raw installed-Windhawk version string; null when unknown.
	windhawkVersion: string | null;
	fsPaths: CoreFsPaths;
};

export type InProcessBackendOptions = {
	services: Services;
	env: CoreEnvironment;
};

export function createInProcessBackend(options: InProcessBackendOptions): WindhawkCore {
	const { services, env } = options;
	const operations: Operations = createOperations(services);
	const currentWindhawkVersion = semver.coerce(env.windhawkVersion, {
		includePrerelease: true,
	});

	return {
		// --- Meta ---

		async getCoreInfo() {
			return {
				contractVersion: CONTRACT_VERSION,
				portable: env.portable,
				arm64Enabled: env.arm64Enabled,
				windhawkVersion: env.windhawkVersion,
				fsPaths: env.fsPaths,
			};
		},

		// --- Pure helpers ---

		async parseModSource(source, language) {
			return parseModSourceWith(services.modSource, source, language);
		},

		async appendToModIdAndName(source, appendToId, appendToName) {
			return services.modSource.appendToIdAndName(source, appendToId, appendToName);
		},

		// --- Installed-mod queries and scoped writes ---

		// Extracted from the extension's getInstalledMods IPC handler (and the
		// CLI's mod list), preserving the call sequence: profile read, metadata
		// scan, config scan, per-mod profile reconciliation, removed-mod
		// cleanup, then a single profile write if anything changed. The write
		// uses asExternalUpdate so the extension's profile watcher forwards
		// the new data to the UI; inert for the CLI.
		async listInstalledMods(params: ListInstalledModsParams): Promise<ListInstalledModsResult> {
			const { language, checkForUpdates, syncProfile } = params;

			const loadErrors: ListInstalledModsResult['loadErrors'] = [];
			const userProfile = services.userProfile.read();
			const modsMetadata = services.modSource.getMetadataOfInstalled(language, (modId, error) => {
				loadErrors.push({ modId, error: String(error) });
			});
			const modsConfig = services.modConfig.getConfigOfInstalled();

			let userProfileUpdated = false;
			const mods: Record<string, InstalledModListEntry> = {};

			for (const modId of new Set([...Object.keys(modsMetadata), ...Object.keys(modsConfig)])) {
				const version = modsMetadata[modId]?.version || '';
				const disabled = modsConfig[modId]?.disabled || false;
				if (syncProfile && !modId.startsWith('local@') &&
					userProfile.updateModDetails(modId, version, disabled)) {
					userProfileUpdated = true;
				}

				const latestVersion = checkForUpdates && userProfile.getModLatestVersion(modId);
				const updateAvailable = !!(latestVersion && latestVersion !== version);
				const userRating = userProfile.getModRating(modId) || 0;
				mods[modId] = {
					metadata: modsMetadata[modId] || null,
					config: modsConfig[modId] || null,
					updateAvailable,
					userRating,
				};
			}

			if (syncProfile) {
				const nonLocalModIds = Object.keys(mods).filter(modId => !modId.startsWith('local@'));
				if (userProfile.cleanupRemovedMods(new Set<string>(nonLocalModIds))) {
					userProfileUpdated = true;
				}

				if (userProfileUpdated) {
					// Set asExternalUpdate so that the extension's file watcher
					// sends the updated data to the UI.
					const asExternalUpdate = true;
					userProfile.write(asExternalUpdate);
				}
			}

			return { mods, loadErrors };
		},

		async getModSource(modId) {
			return services.modSource.getSource(modId);
		},

		async doesModExist(modId) {
			return services.modSource.doesSourceExist(modId) || services.modConfig.doesConfigExist(modId);
		},

		async getModConfig(modId) {
			return services.modConfig.getModConfig(modId);
		},

		async updateModConfig(modId, patch) {
			services.modConfig.setModConfig(modId, patch);
		},

		async getModSettings(modId) {
			return services.modConfig.getModSettings(modId);
		},

		async setModSettings(modId, settings) {
			services.modConfig.setModSettings(modId, settings);
		},

		async setModLoggingEnabled(modId, enable) {
			services.modConfig.enableLogging(modId, enable);
		},

		async setModRating(modId, rating) {
			const userProfile = services.userProfile.read();
			userProfile.setModRating(modId, rating);
			userProfile.write();
		},

		// --- Use-case operations ---

		installMod(input: CoreInstallModInput): AsyncOperation<InstallModResult> {
			const result = operations.installMod({
				...input,
				// The repository folder URL is core-internal; the operations
				// layer still takes it explicitly (and ignores it when
				// compiling locally), exactly as both front-ends passed it.
				modsFolderUrl: services.repoClient.getModsFolderUrl(),
			});
			return {
				result,
				cancel: () => {
					// Cancellation is the compiler's kill-whatever-is-
					// running semantics; at most one compile runs per session.
					// Cancel during the precompiled-download path is a no-op,
					// matching today's behavior.
					services.compiler.cancelCompilation();
					return true;
				},
			};
		},

		compileInstalledMod(input) {
			const result = operations.compileInstalledMod(input);
			return {
				result,
				cancel: () => {
					services.compiler.cancelCompilation();
					return true;
				},
			};
		},

		async setModEnabled(modId, enable) {
			operations.setModEnabled(modId, enable);
		},

		async removeMod(modId) {
			operations.removeMod(modId);
		},

		async applyAppSettings(patch) {
			return operations.applyAppSettings(patch);
		},

		async previewAppSettingsEffects(patch) {
			// Both predicates are pure functions of the patch; nothing is
			// written.
			return {
				requiresRestart: services.appSettings.shouldRestartApp(patch),
				requiresNotify: services.appSettings.shouldNotifyTrayProgram(patch),
			};
		},

		async syncCatalogToProfile(catalog) {
			return operations.syncCatalogToProfile(catalog);
		},

		// --- App settings ---

		async getAppSettings() {
			return services.appSettings.getAppSettings();
		},

		// --- Repository ---

		async fetchCatalog(language) {
			return services.repoClient.fetchCatalog(language);
		},

		async fetchRepoModSource(modId, version) {
			return services.repoClient.fetchModSource(modId, version);
		},

		async fetchModVersions(modId) {
			return services.repoClient.fetchModVersions(modId);
		},

		// --- User profile auxiliary ---

		// Extracted from the extension's _getAppUISettings (the GUI badge) and
		// the CLI's `update status`; one computation serves both.
		async getAppUpdateStatus() {
			const userProfile = services.userProfile.read();
			const latestVersion = userProfile.getAppLatestVersion();
			const latestVersionBleedingEdge = userProfile.getAppLatestVersionBleedingEdge();
			const latest = semver.coerce(latestVersion, { includePrerelease: true });
			const latestBleedingEdge = semver.coerce(latestVersionBleedingEdge, {
				includePrerelease: true,
			});
			return {
				latestVersion,
				latestVersionBleedingEdge,
				updateAvailable: !!(currentWindhawkVersion && latest &&
					semver.lt(currentWindhawkVersion, latest)),
				updateAvailableBleedingEdge: !!(currentWindhawkVersion && latestBleedingEdge &&
					semver.lt(currentWindhawkVersion, latestBleedingEdge)),
			};
		},

		async getProfileWatchInfo() {
			return {
				filePath: services.userProfile.getFilePath(),
				lastModifiedByUserMtimeMs: services.userProfile.getLastModifiedByUserMtimeMs(),
			};
		},

		// --- Tray ---

		async notifyTray(action) {
			switch (action) {
				case 'restartBg':
					services.trayProgram.postAppRestartBg();
					break;
				case 'newUpdatesFound':
					services.trayProgram.postNewUpdatesFound();
					break;
				case 'appSettingsChanged':
					services.trayProgram.postAppSettingsChanged();
					break;
			}
		},

		// --- Update ---

		startUpdate(events) {
			const result = services.update.startUpdate({
				onProgress: events.onProgress,
				onInstalling: events.onInstalling,
			});
			return {
				result,
				cancel: () => services.update.cancelUpdate(),
			};
		},

		// --- Editor support ---

		// The clangd flags for compile_flags.txt. The DLL single-sources this
		// with the real compiler flags; the in-process backend returns the same
		// fixed set the editor workspace writes
		// today (editorWorkspaceUtils.initializeEditorSettings).
		async getCompileFlags() {
			return [
				'-x',
				'c++',
				'-std=c++23',
				'-target',
				'x86_64-w64-mingw32',
				'-DUNICODE',
				'-D_UNICODE',
				'-DWINVER=0x0A00',
				'-D_WIN32_WINNT=0x0A00',
				'-D_WIN32_IE=0x0A00',
				'-DNTDDI_VERSION=0x0A000008',
				'-D__USE_MINGW_ANSI_STDIO=0',
				'-DWH_MOD',
				'-DWH_EDITING',
				'-include',
				'windhawk_api.h',
				'-Wall',
				'-Wextra',
				'-Wno-unused-parameter',
				'-Wno-missing-field-initializers',
				'-Wno-cast-function-type-mismatch',
			];
		},
	};
}
