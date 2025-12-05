import * as fs from 'fs';
import fetch from 'node-fetch';
import * as path from 'path';
import * as semver from 'semver';
import * as vscode from 'vscode';
import * as i18n from 'vscode-nls-i18n';
import config from './config';
import { WindhawkLogOutput } from './logOutputChannel';
import * as storagePaths from './storagePaths';
import { AppSettings, AppSettingsUtils, AppSettingsUtilsNonPortable, AppSettingsUtilsPortable } from './utils/appSettingsUtils';
import CompilerUtils, { CompilerError } from './utils/compilerUtils';
import EditorWorkspaceUtils from './utils/editorWorkspaceUtils';
import { ModConfigUtils, ModConfigUtilsNonPortable, ModConfigUtilsPortable } from './utils/modConfigUtils';
import ModFilesUtils from './utils/modFilesUtils';
import ModSourceUtils from './utils/modSourceUtils';
import TrayProgramUtils from './utils/trayProgramUtils';
import { UpdateUtils } from './utils/updateUtils';
import UserProfileUtils, { UserProfile } from './utils/userProfileUtils';
import * as webviewIPC from './webviewIPC';
import {
	AppUISettings,
	CompileEditedModData,
	CompileModData,
	CompileModReplyData,
	DeleteModData,
	EditModData,
	EnableEditedModData,
	EnableEditedModLoggingData,
	EnableModData,
	ExitEditorModeData,
	ForkModData,
	GetFeaturedModsReplyData,
	GetInstalledModsReplyData,
	GetModConfigData,
	GetModSettingsData,
	GetModSourceDataData,
	GetModVersionsData,
	GetModVersionsReplyData,
	GetRepositoryModSourceDataData,
	GetRepositoryModsReplyData,
	InstallModData,
	InstallModReplyData,
	ModConfig,
	ModMetadata,
	SetModSettingsData,
	StartUpdateReplyData,
	UpdateAppSettingsData,
	UpdateInstalledModsDetailsData,
	UpdateModConfigData,
	UpdateModRatingData
} from './webviewIPCMessages';

type AppUtils = {
	modSource: ModSourceUtils,
	modConfig: ModConfigUtils,
	modFiles: ModFilesUtils,
	compiler: CompilerUtils,
	editorWorkspace: EditorWorkspaceUtils,
	trayProgram: TrayProgramUtils,
	userProfile: UserProfileUtils,
	appSettings: AppSettingsUtils,
	update: UpdateUtils
};

// Set to a local folder to use a dev environment.
// Set to null to use the 'webview' folder.
const baseDebugReactUiPath: string | null = config.debug.reactProjectBuildPath;

const currentWindhawkVersion = semver.coerce(
	vscode.extensions.getExtension('m417z.windhawk')?.packageJSON.version
);

let windhawkLogOutput: WindhawkLogOutput | null = null;
let windhawkCompilerOutput: vscode.OutputChannel | null = null;

export function activate(context: vscode.ExtensionContext) {
	if (!config.debug.disableEnvVarCheck && !process.env.WINDHAWK_UI_PATH) {
		vscode.window.showErrorMessage('Windhawk: Unsupported environment, perhaps VSCode was launched directly');
		return;
	}

	try {
		i18n.init(context.extensionPath);

		windhawkLogOutput = new WindhawkLogOutput(path.join(context.extensionPath, 'files', 'DbgViewMini.exe'));
		windhawkCompilerOutput = vscode.window.createOutputChannel('Windhawk Compiler');

		const arm64Enabled = process.env.WINDHAWK_ARM64_ENABLED === '1';

		const paths = storagePaths.getStoragePaths();
		const { appRootPath, appDataPath, enginePath, compilerPath } = paths.fsPaths;
		const utils: AppUtils = {
			modSource: new ModSourceUtils(appDataPath),
			modConfig: paths.portable
				? new ModConfigUtilsPortable(appDataPath)
				: new ModConfigUtilsNonPortable(paths.regKey, paths.regSubKey, appDataPath),
			modFiles: new ModFilesUtils(appDataPath, arm64Enabled, currentWindhawkVersion),
			compiler: new CompilerUtils(compilerPath, enginePath, appDataPath, arm64Enabled),
			editorWorkspace: new EditorWorkspaceUtils(),
			trayProgram: new TrayProgramUtils(appRootPath),
			userProfile: new UserProfileUtils(appDataPath),
			appSettings: paths.portable
				? new AppSettingsUtilsPortable(appDataPath)
				: new AppSettingsUtilsNonPortable(paths.regKey, paths.regSubKey),
			update: new UpdateUtils(paths.portable, appRootPath)
		};

		const sidebarWebviewViewProvider = new WindhawkViewProvider(context.extensionUri, context.extensionPath, utils);

		context.subscriptions.push(
			vscode.window.registerWebviewViewProvider(WindhawkViewProvider.viewType, sidebarWebviewViewProvider)
		);

		context.subscriptions.push(
			vscode.workspace.onDidChangeTextDocument(({ contentChanges, document }) => {
				if (contentChanges.length > 0) {
					sidebarWebviewViewProvider.fileWasModified(document);
				}
			})
		);

		const onEnterEditorMode = (modId: string, modWasModified = false) => {
			sidebarWebviewViewProvider.setEditedMod(modId, modWasModified);
		};

		const onAppSettingsUpdated = () => {
			sidebarWebviewViewProvider.appSettingsUpdated();
		};

		context.subscriptions.push(
			vscode.commands.registerCommand('windhawk.start', (options?: WindhawkPanelOptions) => {
				WindhawkPanel.createOrShow(context.extensionUri, context.extensionPath, utils, {
					onEnterEditorMode,
					onAppSettingsUpdated
				}, {
					title: '',
					...options
				});
			}),
			vscode.commands.registerCommand('windhawk.compileMod', () => {
				sidebarWebviewViewProvider.compileMod();
			}),
		);

		utils.editorWorkspace.restoreEditorMode().then(({ modId, modWasModified }) => {
			if (modId) {
				sidebarWebviewViewProvider.setEditedMod(modId, !!modWasModified);
			}
		}).catch(e => reportException(e));

		const onUserProfileModified = () => {
			const { mtimeMs } = fs.statSync(utils.userProfile.getFilePath());
			if (mtimeMs !== utils.userProfile.getLastModifiedByUserMtimeMs()) {
				WindhawkPanel.userProfileChanged();
			}
		};

		const userProfileWatcher = vscode.workspace.createFileSystemWatcher(
			new vscode.RelativePattern(vscode.Uri.file(utils.userProfile.getFilePath()), '*'));
		userProfileWatcher.onDidCreate(onUserProfileModified);
		userProfileWatcher.onDidChange(onUserProfileModified);
		context.subscriptions.push(userProfileWatcher);
	} catch (e) {
		reportException(e);
	}
}

type RepositoryModsType = Record<string, any>;

type WindhawkPanelCallbacks = {
	onEnterEditorMode: (modId: string, modWasModified: boolean) => void,
	onAppSettingsUpdated: () => void
};

type WindhawkPanelParams = {
	previewModId?: string
};

type WindhawkPanelOptions = {
	title: string,
	createColumn?: vscode.ViewColumn,
	params?: WindhawkPanelParams
};

/**
 * Manages Windhawk webview panels.
 */
class WindhawkPanel {
	/**
	 * Track the currently panel. Only allow a single panel to exist at a time.
	 */
	public static currentPanel: WindhawkPanel | undefined;

	public static readonly viewType = 'windhawk';

	private readonly _panel: vscode.WebviewPanel;
	private readonly _extensionUri: vscode.Uri;
	private readonly _extensionPath: string;
	private readonly _utils: AppUtils;
	private readonly _callbacks: WindhawkPanelCallbacks;
	private _disposables: vscode.Disposable[] = [];
	private _language = 'en';
	private _checkForUpdates = true;
	private _alwaysCompileModsLocally = false;

	public static createOrShow(
		extensionUri: vscode.Uri,
		extensionPath: string,
		utils: AppUtils,
		callbacks: WindhawkPanelCallbacks,
		options: WindhawkPanelOptions
	) {
		const column = vscode.window.activeTextEditor
			? vscode.window.activeTextEditor.viewColumn
			: undefined;

		// If we already have a panel, refresh and show it.
		if (WindhawkPanel.currentPanel) {
			WindhawkPanel.currentPanel.refresh(options.title, options.params);
			WindhawkPanel.currentPanel._panel.reveal();
			return;
		}

		// Otherwise, create a new panel.
		const localResourceRoots = [vscode.Uri.joinPath(extensionUri, 'webview')];
		if (baseDebugReactUiPath) {
			localResourceRoots.push(vscode.Uri.file(baseDebugReactUiPath));
		}

		const panel = vscode.window.createWebviewPanel(
			WindhawkPanel.viewType,
			options.title,
			options.createColumn || column || vscode.ViewColumn.One,
			{
				// Enable javascript in the webview.
				enableScripts: true,

				// And restrict the webview to only loading content from our extension's `webview` directory.
				localResourceRoots
			}
		);

		WindhawkPanel.currentPanel = new WindhawkPanel(panel, extensionUri, extensionPath, utils, callbacks, options.params);
	}

	public static refreshIfExists(title: string, params?: WindhawkPanelParams) {
		WindhawkPanel.currentPanel?.refresh(title, params);
	}

	private constructor(
		panel: vscode.WebviewPanel,
		extensionUri: vscode.Uri,
		extensionPath: string,
		utils: AppUtils,
		callbacks: WindhawkPanelCallbacks,
		params?: WindhawkPanelParams
	) {
		this._panel = panel;
		this._extensionUri = extensionUri;
		this._extensionPath = extensionPath;
		this._utils = utils;
		this._callbacks = callbacks;

		// Set the webview initial html content and icon.
		this._panel.webview.html = this._getHtmlForWebview(this._panel.webview, params);
		this._panel.iconPath = {
			light: vscode.Uri.joinPath(extensionUri, 'assets', 'tab-icon-black.svg'),
			dark: vscode.Uri.joinPath(extensionUri, 'assets', 'tab-icon-white.svg')
		};

		// Listen for when the panel is disposed.
		// This happens when the user closes the panel or when the panel is closed programmatically.
		this._panel.onDidDispose(() => this.dispose(), null, this._disposables);

		// Handle messages from the webview.
		this._panel.webview.onDidReceiveMessage(
			message => this._handleMessage(message),
			null,
			this._disposables
		);
	}

	public refresh(title: string, params?: WindhawkPanelParams) {
		this._panel.title = title;

		// To refresh, first clear the html.
		this._panel.webview.html = '';
		this._panel.webview.html = this._getHtmlForWebview(this._panel.webview, params);
	}

	public static userProfileChanged() {
		// If we don't already have a panel, there's nothing to update.
		if (!WindhawkPanel.currentPanel) {
			return;
		}

		WindhawkPanel.currentPanel._userProfileChanged();
	}

	public dispose() {
		WindhawkPanel.currentPanel = undefined;

		// Clean up our resources.
		this._panel.dispose();

		while (this._disposables.length) {
			const x = this._disposables.pop();
			if (x) {
				x.dispose();
			}
		}
	}

	private _getHtmlForWebview(webview: vscode.Webview, params?: WindhawkPanelParams) {
		const webviewPathOnDisk = baseDebugReactUiPath
			? vscode.Uri.file(baseDebugReactUiPath)
			: vscode.Uri.joinPath(this._extensionUri, 'webview');

		const baseWebviewUri = webview.asWebviewUri(webviewPathOnDisk);
		let html = fs.readFileSync(vscode.Uri.joinPath(webviewPathOnDisk, 'index.html').fsPath, 'utf8');

		const csp = [
			`default-src 'none'`,
			`style-src 'unsafe-inline' ${webview.cspSource}`,
			`img-src ${webview.cspSource} data: https://i.imgur.com https://raw.githubusercontent.com https://mods.windhawk.net`,
			`script-src ${webview.cspSource} blob:`,
			`connect-src ${webview.cspSource} https://mods.windhawk.net https://ramensoftware.com`,
			`font-src ${webview.cspSource}`
		];

		html = html.replace('<head>', `<head>
			<base href="${baseWebviewUri.toString()}/">
			<meta http-equiv="Content-Security-Policy" content="${csp.join('; ')};">
		`);

		const dataParams = params ? ` data-params="${escapeHtml(JSON.stringify(params))}"` : '';

		html = html.replace(/<body([^>]*)>/, `<body data-content="panel"${dataParams}$1>`);

		return html;
	}

	private _getAppUISettings(appSettings: AppSettings, userProfile?: UserProfile): AppUISettings {
		let updateIsAvailable = false;
		if (!appSettings.disableUpdateCheck) {
			try {
				const currentVersion = currentWindhawkVersion;
				const latestVersion = semver.coerce((userProfile || this._utils.userProfile.read()).getAppLatestVersion());
				updateIsAvailable = !!(currentVersion && latestVersion && semver.lt(currentVersion, latestVersion));
			} catch (e) {
				reportException(e);
			}
		}

		return {
			language: appSettings.language,
			devModeOptOut: appSettings.devModeOptOut,
			devModeUsedAtLeastOnce: appSettings.devModeUsedAtLeastOnce,
			loggingEnabled: (
				appSettings.loggingVerbosity > 0 ||
				appSettings.engine.loggingVerbosity > 0
			),
			updateIsAvailable,
			safeMode: appSettings.safeMode
		};
	}

	private _userProfileChanged() {
		try {
			const userProfile = this._utils.userProfile.read();

			// First, recalculate UI settings, since the update availability value
			// depends on the user profile.
			const appSettings = this._utils.appSettings.getAppSettings();
			this._language = appSettings.language;
			this._checkForUpdates = !appSettings.disableUpdateCheck;
			this._alwaysCompileModsLocally = appSettings.alwaysCompileModsLocally;

			webviewIPC.setNewAppSettings(this._panel.webview, {
				appUISettings: this._getAppUISettings(appSettings, userProfile)
			});

			// Next, recalculate mod values which depend on the user profile.
			const details: UpdateInstalledModsDetailsData['details'] = {};

			const modsMetadata = this._utils.modSource.getMetadataOfInstalled(this._language, (modId, error) => {
				vscode.window.showErrorMessage(`Failed to load mod ${modId}: ${error}`);
			});
			const modsConfig = this._utils.modConfig.getConfigOfInstalled();

			for (const modId of new Set([...Object.keys(modsMetadata), ...Object.keys(modsConfig)])) {
				const modLatestVersion = this._checkForUpdates && userProfile.getModLatestVersion(modId);
				const updateAvailable = !!(modLatestVersion && modLatestVersion !== (modsMetadata[modId]?.version || ''));
				const userRating = userProfile.getModRating(modId) || 0;
				details[modId] = {
					updateAvailable,
					userRating: userRating
				};
			}

			webviewIPC.updateInstalledModsDetails(this._panel.webview, {
				details
			});
		} catch (e) {
			reportException(e);
		}
	}

	private readonly _handleMessageMap: Record<string, (message: any) => void> = {
		getInitialAppSettings: message => {
			let appUISettings: Partial<AppUISettings> = {};
			try {
				const appSettings = this._utils.appSettings.getAppSettings();
				this._language = appSettings.language;
				this._checkForUpdates = !appSettings.disableUpdateCheck;
				this._alwaysCompileModsLocally = appSettings.alwaysCompileModsLocally;

				appUISettings = this._getAppUISettings(appSettings);
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getInitialAppSettingsReply(this._panel.webview, message.messageId, {
				appUISettings
			});
		},
		getInstalledMods: message => {
			const installedMods: GetInstalledModsReplyData['installedMods'] = {};
			try {
				const userProfile = this._utils.userProfile.read();
				const modsMetadata = this._utils.modSource.getMetadataOfInstalled(this._language, (modId, error) => {
					vscode.window.showErrorMessage(`Failed to load mod ${modId}: ${error}`);
				});
				const modsConfig = this._utils.modConfig.getConfigOfInstalled();

				for (const modId of new Set([...Object.keys(modsMetadata), ...Object.keys(modsConfig)])) {
					const modLatestVersion = this._checkForUpdates && userProfile.getModLatestVersion(modId);
					const updateAvailable = !!(modLatestVersion && modLatestVersion !== (modsMetadata[modId]?.version || ''));
					const userRating = userProfile.getModRating(modId) || 0;
					installedMods[modId] = {
						metadata: modsMetadata[modId] || null,
						config: modsConfig[modId] || null,
						updateAvailable,
						userRating: userRating
					};
				}
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getInstalledModsReply(this._panel.webview, message.messageId, {
				installedMods
			});
		},
		getFeaturedMods: async message => {
			let featuredMods: GetFeaturedModsReplyData['featuredMods'] = null;
			try {
				const repositoryMods = await this._fetchRepositoryMods(this._language);
				featuredMods = Object.fromEntries(
					Object.entries(repositoryMods).filter(([k, v]) => v.featured));
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getFeaturedModsReply(this._panel.webview, message.messageId, {
				featuredMods
			});
		},
		getRepositoryMods: async message => {
			let mods: GetRepositoryModsReplyData['mods'] = null;
			try {
				const repositoryMods = await this._fetchRepositoryMods(this._language);

				mods = {};
				for (const [modId, value] of Object.entries(repositoryMods)) {
					mods[modId] = {
						repository: value
					};
				}

				const userProfile = this._utils.userProfile.read();
				const modsMetadata = this._utils.modSource.getMetadataOfInstalled(this._language, (modId, error) => {
					vscode.window.showErrorMessage(`Failed to load mod ${modId}: ${error}`);
				});
				const modsConfig = this._utils.modConfig.getConfigOfInstalled();

				for (const modId of new Set([...Object.keys(modsMetadata), ...Object.keys(modsConfig)])) {
					if (mods[modId]) {
						const userRating = userProfile.getModRating(modId) || 0;
						mods[modId].installed = {
							metadata: modsMetadata[modId] || null,
							config: modsConfig[modId] || null,
							userRating
						};
					}
				}
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getRepositoryModsReply(this._panel.webview, message.messageId, {
				mods
			});
		},
		getModSourceData: message => {
			const data: GetModSourceDataData = message.data;

			let source: string | null = null;
			try {
				source = this._utils.modSource.getSource(data.modId);
			} catch (e) {
				reportException(e);
			}

			let metadata: ModMetadata | null = null;
			let readme: string | null = null;
			let initialSettings: Record<string, any>[] | null = null;
			if (source) {
				try {
					metadata = this._utils.modSource.extractMetadata(source, this._language);
				} catch (e) {
					reportException(e);
				}

				try {
					readme = this._utils.modSource.extractReadme(source);
				} catch (e) {
					reportException(e);
				}

				try {
					initialSettings = this._utils.modSource.extractInitialSettings(source, this._language);
				} catch (e) {
					reportException(e);
				}
			}

			webviewIPC.getModSourceDataReply(this._panel.webview, message.messageId, {
				modId: data.modId,
				data: {
					source,
					metadata,
					readme,
					initialSettings
				}
			});
		},
		getRepositoryModSourceData: async message => {
			const data: GetRepositoryModSourceDataData = message.data;

			// Construct URL: if version is provided, use versioned path,
			// otherwise use latest.
			const url = data.version
				? `${config.urls.modsFolder}${data.modId}/${data.version}.wh.cpp`
				: `${config.urls.modsFolder}${data.modId}.wh.cpp`;

			let source: string | null = null;
			try {
				const response = await fetch(url);
				if (!response.ok) {
					throw Error('Server error: ' + (response.statusText || response.status));
				}

				source = await response.text();

				// Make sure the source code has CRLF newlines.
				source = source.replace(/\r\n|\r|\n/g, '\r\n');
			} catch (e) {
				reportException(e);
			}

			let metadata: ModMetadata | null = null;
			let readme: string | null = null;
			let initialSettings: Record<string, any>[] | null = null;
			if (source) {
				try {
					metadata = this._utils.modSource.extractMetadata(source, this._language);
				} catch (e) {
					reportException(e);
				}

				try {
					readme = this._utils.modSource.extractReadme(source);
				} catch (e) {
					reportException(e);
				}

				try {
					initialSettings = this._utils.modSource.extractInitialSettings(source, this._language);
				} catch (e) {
					reportException(e);
				}
			}

			webviewIPC.getRepositoryModSourceDataReply(this._panel.webview, message.messageId, {
				modId: data.modId,
				version: data.version,
				data: {
					source,
					metadata,
					readme,
					initialSettings
				}
			});
		},
		getModVersions: async message => {
			const data: GetModVersionsData = message.data;
			const { modId } = data;
			const url = `${config.urls.modsFolder}${modId}/versions.json`;

			let versions: GetModVersionsReplyData['versions'] = [];
			try {
				const response = await fetch(url);
				if (!response.ok) {
					throw Error('Server error: ' + (response.statusText || response.status));
				}

				const jsonData = await response.json();
				versions = jsonData.map((v: any) => ({
					version: v.version,
					timestamp: v.timestamp,
					isPreRelease: v.version.includes('-')
				}));
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getModVersionsReply(this._panel.webview, message.messageId, {
				modId,
				versions
			});
		},
		getModSettings: message => {
			const data: GetModSettingsData = message.data;

			let settings: Record<string, any> = {};
			try {
				settings = this._utils.modConfig.getModSettings(data.modId);
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getModSettingsReply(this._panel.webview, message.messageId, {
				modId: data.modId,
				settings
			});
		},
		setModSettings: message => {
			const data: SetModSettingsData = message.data;

			let succeeded = false;
			try {
				this._utils.modConfig.setModSettings(data.modId, data.settings);

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.setModSettingsReply(this._panel.webview, message.messageId, {
				modId: data.modId,
				succeeded
			});
		},
		getModConfig: message => {
			const data: GetModConfigData = message.data;

			let config: ModConfig | null = null;
			try {
				config = this._utils.modConfig.getModConfig(data.modId);
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getModConfigReply(this._panel.webview, message.messageId, {
				modId: data.modId,
				config
			});
		},
		updateModConfig: message => {
			const data: UpdateModConfigData = message.data;

			let succeeded = false;
			try {
				this._utils.modConfig.setModConfig(data.modId, data.config);

				webviewIPC.setNewModConfig(this._panel.webview, {
					modId: data.modId,
					config: data.config
				});

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.updateModConfigReply(this._panel.webview, message.messageId, {
				modId: data.modId,
				succeeded
			});
		},
		installMod: async message => {
			const data: InstallModData = message.data;

			let installedModDetails: InstallModReplyData['installedModDetails'] = null;

			try {
				windhawkCompilerOutput?.clear();
				windhawkCompilerOutput?.hide();

				const modId = data.modId;
				const modSource = data.modSource;
				const disabled = !!data.disabled;

				const metadata = this._utils.modSource.extractMetadata(modSource, this._language);
				if (!metadata.id) {
					throw new Error('Mod id must be specified in the source code');
				} else if (metadata.id !== modId) {
					throw new Error('Mod id specified in the source code doesn\'t match');
				}

				const initialSettings = this._utils.modSource.extractInitialSettingsForEngine(modSource);

				let previousInitialSettings: Record<string, string | number> | undefined;
				try {
					const prev = this._utils.modSource.extractInitialSettingsForEngine(
						this._utils.modSource.getSource(modId)
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
				if (this._alwaysCompileModsLocally) {
					const result = await this._utils.compiler.compileMod(
						modId,
						metadata.version || '',
						metadata.include || [],
						modSource,
						metadata.architecture || [],
						metadata.compilerOptions
					);
					targetDllName = result.targetDllName;
				} else {
					const result = await this._utils.modFiles.downloadPrecompiledMod(
						modId,
						metadata.version || '',
						metadata.architecture || [],
						config.urls.modsFolder
					);
					targetDllName = result.targetDllName;
				}

				this._utils.modConfig.setModConfig(modId, {
					libraryFileName: targetDllName,
					disabled,
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
				}, {
					initialSettings: initialSettings || {},
					previousInitialSettings
				});

				this._utils.modSource.setSource(modId, modSource);

				this._utils.modFiles.deleteOldModFiles(modId, metadata.architecture || [], targetDllName);

				const userProfile = this._utils.userProfile.read();
				userProfile.setModVersion(modId, metadata.version || '');
				userProfile.write();

				const modConfig = this._utils.modConfig.getModConfig(modId);
				if (!modConfig) {
					throw new Error('Failed to query installed mod details');
				}

				installedModDetails = {
					metadata,
					config: modConfig
				};
			} catch (e) {
				reportCompilerException(e, true);
			}

			webviewIPC.installModReply(this._panel.webview, message.messageId, {
				modId: data.modId,
				installedModDetails
			});
		},
		compileMod: async message => {
			const data: CompileModData = message.data;

			let compiledModDetails: CompileModReplyData['compiledModDetails'] = null;

			try {
				windhawkCompilerOutput?.clear();
				windhawkCompilerOutput?.hide();

				const modId = data.modId;
				const modSource = this._utils.modSource.getSource(modId);
				const disabled = !!data.disabled;

				const metadata = this._utils.modSource.extractMetadata(modSource, this._language);
				if (!metadata.id) {
					throw new Error('Mod id must be specified in the source code');
				} else if (metadata.id !== modId.replace(/^local@/, '')) {
					throw new Error('Mod id specified in the source code doesn\'t match');
				}

				const { targetDllName } = await this._utils.compiler.compileMod(
					modId,
					metadata.version || '',
					metadata.include || [],
					modSource,
					metadata.architecture || [],
					metadata.compilerOptions
				);

				this._utils.modConfig.setModConfig(modId, {
					libraryFileName: targetDllName,
					disabled,
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

				this._utils.modFiles.deleteOldModFiles(modId, metadata.architecture || [], targetDllName);

				const config = this._utils.modConfig.getModConfig(modId);
				if (!config) {
					throw new Error('Failed to query compiled mod details');
				}

				compiledModDetails = {
					metadata,
					config
				};
			} catch (e) {
				reportCompilerException(e, true);
			}

			webviewIPC.compileModReply(this._panel.webview, message.messageId, {
				modId: data.modId,
				compiledModDetails
			});
		},
		enableMod: message => {
			const data: EnableModData = message.data;

			let succeeded = false;
			try {
				const modId: string = data.modId;
				const enable: boolean = data.enable;

				this._utils.modConfig.enableMod(modId, enable);

				if (!modId.startsWith('local@')) {
					const userProfile = this._utils.userProfile.read();
					userProfile.setModDisabled(modId, !enable);
					userProfile.write();
				}

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.enableModReply(this._panel.webview, message.messageId, {
				modId: data.modId,
				enabled: data.enable,
				succeeded
			});
		},
		createNewMod: async message => {
			try {
				const modSourcePath = path.join(this._extensionPath, 'files', 'mod_template.wh.cpp');
				let modSource = fs.readFileSync(modSourcePath, 'utf8');

				const metadata = this._utils.modSource.extractMetadata(modSource, this._language);
				if (!metadata.id) {
					throw new Error('Mod id must be specified in the source code');
				}

				let newModId = metadata.id;
				let localModId = 'local@' + newModId;
				if (this._utils.modSource.doesSourceExist(localModId) || this._utils.modConfig.doesConfigExist(localModId)) {
					let counter = 2;
					let modIdSuffix;
					for (; ;) {
						modIdSuffix = '-' + counter;
						newModId = metadata.id + modIdSuffix;
						localModId = 'local@' + newModId;

						const exists = this._utils.modSource.doesSourceExist(localModId) || this._utils.modConfig.doesConfigExist(localModId);
						if (!exists) {
							break;
						}

						counter++;
					}

					const modNameSuffix = ` (${counter})`;
					modSource = this._utils.modSource.appendToIdAndName(modSource, modIdSuffix, modNameSuffix);
				}

				this._utils.editorWorkspace.initializeFromModSource(modSource);

				this._callbacks.onEnterEditorMode(newModId, false);

				await this._utils.editorWorkspace.enterEditorMode(newModId);
			} catch (e) {
				reportException(e);
			}
		},
		editMod: async message => {
			const data: EditModData = message.data;

			try {
				const modSource = this._utils.modSource.getSource(data.modId);

				const metadata = this._utils.modSource.extractMetadata(modSource, this._language);
				if (!metadata.id) {
					throw new Error('Mod id must be specified in the source code');
				}

				const modSourceFromDrafts = this._utils.editorWorkspace.loadModFromDrafts(metadata.id);
				if (modSourceFromDrafts) {
					this._utils.editorWorkspace.deleteModFromDrafts(metadata.id);
				}

				this._utils.editorWorkspace.initializeFromModSource(modSource, modSourceFromDrafts);

				this._callbacks.onEnterEditorMode(metadata.id, !!modSourceFromDrafts);

				await this._utils.editorWorkspace.enterEditorMode(metadata.id, !!modSourceFromDrafts);
			} catch (e) {
				reportException(e);
			}
		},
		forkMod: async message => {
			const data: ForkModData = message.data;

			try {
				let modSource = data.modSource || this._utils.modSource.getSource(data.modId);

				const metadata = this._utils.modSource.extractMetadata(modSource, this._language);
				if (!metadata.id) {
					throw new Error('Mod id must be specified in the source code');
				} else if (metadata.id !== data.modId.replace(/^local@/, '')) {
					throw new Error('Mod id specified in the source code doesn\'t match');
				}

				let modIdSuffix = '-fork';
				let forkModId = metadata.id + modIdSuffix;
				let localModId = 'local@' + forkModId;
				let modNameSuffix = ' - Fork';
				if (this._utils.modSource.doesSourceExist(localModId) || this._utils.modConfig.doesConfigExist(localModId)) {
					let counter = 2;
					for (; ;) {
						modIdSuffix = '-fork' + counter;
						forkModId = metadata.id + modIdSuffix;
						localModId = 'local@' + forkModId;

						const exists = this._utils.modSource.doesSourceExist(localModId) || this._utils.modConfig.doesConfigExist(localModId);
						if (!exists) {
							break;
						}

						counter++;
					}

					modNameSuffix = ` - Fork (${counter})`;
				}

				modSource = this._utils.modSource.appendToIdAndName(modSource, modIdSuffix, modNameSuffix);

				this._utils.editorWorkspace.initializeFromModSource(modSource);

				this._callbacks.onEnterEditorMode(forkModId, false);

				await this._utils.editorWorkspace.enterEditorMode(forkModId);
			} catch (e) {
				reportException(e);
			}
		},
		deleteMod: message => {
			const data: DeleteModData = message.data;

			let succeeded = false;
			try {
				const modId: string = data.modId;

				this._utils.modConfig.deleteMod(modId);
				this._utils.modSource.deleteSource(modId);

				this._utils.modFiles.deleteModFiles(modId);

				if (modId.startsWith('local@')) {
					this._utils.editorWorkspace.deleteModFromDrafts(modId.replace(/^local@/, ''));
				} else {
					const userProfile = this._utils.userProfile.read();
					userProfile.deleteMod(modId);
					userProfile.write();
				}

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.deleteModReply(this._panel.webview, message.messageId, {
				modId: data.modId,
				succeeded
			});
		},
		updateModRating: message => {
			const data: UpdateModRatingData = message.data;

			let succeeded = false;

			try {
				const userProfile = this._utils.userProfile.read();
				userProfile.setModRating(data.modId, data.rating);
				userProfile.write();

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.updateModRatingReply(this._panel.webview, message.messageId, {
				modId: data.modId,
				rating: data.rating,
				succeeded
			});
		},
		getAppSettings: message => {
			let appSettings: Partial<AppSettings> = {};
			try {
				appSettings = this._utils.appSettings.getAppSettings();
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getAppSettingsReply(this._panel.webview, message.messageId, {
				appSettings
			});
		},
		updateAppSettings: message => {
			const data: UpdateAppSettingsData = message.data;

			let succeeded = false;
			try {
				const appSettings: Partial<AppSettings> = data.appSettings;

				this._utils.appSettings.updateAppSettings(appSettings);

				const newAppSettings = this._utils.appSettings.getAppSettings();
				this._language = newAppSettings.language;
				this._checkForUpdates = !newAppSettings.disableUpdateCheck;
				this._alwaysCompileModsLocally = newAppSettings.alwaysCompileModsLocally;

				webviewIPC.setNewAppSettings(this._panel.webview, {
					appUISettings: this._getAppUISettings(newAppSettings)
				});

				this._callbacks.onAppSettingsUpdated();

				if (this._utils.appSettings.shouldRestartApp(appSettings)) {
					this._utils.trayProgram.postAppRestartBg();
					vscode.window.showInformationMessage('Windhawk was restarted');
				} else if (this._utils.appSettings.shouldNotifyTrayProgram(appSettings)) {
					this._utils.trayProgram.postAppSettingsChanged();
				}

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.updateAppSettingsReply(this._panel.webview, message.messageId, {
				appSettings: data.appSettings,
				succeeded
			});
		},
		showAdvancedDebugLogOutput: message => {
			try {
				windhawkLogOutput?.createOrShow();
			} catch (e) {
				reportException(e);
			}
		},
		startUpdate: async message => {
			let result: StartUpdateReplyData = {
				succeeded: false,
				error: 'Update failed'
			};

			try {
				result = await this._utils.update.startUpdate({
					onProgress: (data) => {
						webviewIPC.updateDownloadProgress(this._panel.webview, data);
					},
					onInstalling: () => {
						webviewIPC.updateInstalling(this._panel.webview, {});
					}
				});
			} catch (e) {
				reportException(e);
			}

			webviewIPC.startUpdateReply(this._panel.webview, message.messageId, result);
		},
		cancelUpdate: message => {
			let succeeded = false;
			try {
				if (this._utils.update.cancelUpdate()) {
					succeeded = true;
				}
			} catch (e) {
				reportException(e);
			}

			webviewIPC.cancelUpdateReply(this._panel.webview, message.messageId, {
				succeeded
			});
		}
	};

	private _handleMessage(message: any) {
		const { command, ...rest } = message;
		this._handleMessageMap[command](rest);
	}

	private async _fetchRepositoryMods(language: string) {
		const languageCatalogUrl = config.urls.modsUrlRoot + 'catalogs/' + language + '.json';
		let response = await fetch(languageCatalogUrl);
		if (response.status === 404) {
			// Fallback to the default catalog if the language one is not available.
			const defaultCatalogUrl = config.urls.modsUrlRoot + 'catalog.json';
			response = await fetch(defaultCatalogUrl);
		}

		if (!response.ok) {
			throw Error('Server error: ' + (response.statusText || response.status));
		}

		const data = await response.json();
		this._updateUserProfileJson(data);
		return data.mods as RepositoryModsType;
	}

	private _updateUserProfileJson(data: any) {
		const userProfile = this._utils.userProfile.read();

		const appLatestVersion = data.app.version;

		const repositoryMods: RepositoryModsType = data.mods;
		const modLatestVersion: Record<string, string> = {};
		for (const [modId, value] of Object.entries(repositoryMods)) {
			const { version } = value.metadata;
			if (version) {
				modLatestVersion[modId] = version;
			}
		}

		if (userProfile.updateLatestVersions(appLatestVersion, modLatestVersion)) {
			// Set asExternalUpdate so that the file watcher will send the
			// updated data to the UI.
			const asExternalUpdate = true;
			userProfile.write(asExternalUpdate);

			if (this._checkForUpdates) {
				this._utils.trayProgram.postNewUpdatesFound();
			}
		}
	}
}

class WindhawkViewProvider implements vscode.WebviewViewProvider {
	public static readonly viewType = 'windhawk.sidebar';

	private _view?: vscode.WebviewView;
	private readonly _extensionUri: vscode.Uri;
	private readonly _extensionPath: string;
	private readonly _utils: AppUtils;
	private _language = 'en';
	private _editedModId?: string;
	private _editedModWasModified = false;
	private _editedModModifiedCounter = 0;
	private _editedModBeingCompiled = false;
	private _editedModCompilationFailed = false;

	constructor(
		extensionUri: vscode.Uri,
		extensionPath: string,
		utils: AppUtils
	) {
		this._extensionUri = extensionUri;
		this._extensionPath = extensionPath;
		this._utils = utils;
	}

	public resolveWebviewView(
		webviewView: vscode.WebviewView,
		context: vscode.WebviewViewResolveContext,
		_token: vscode.CancellationToken,
	) {
		this._view = webviewView;

		const localResourceRoots = [vscode.Uri.joinPath(this._extensionUri, 'webview')];
		if (baseDebugReactUiPath) {
			localResourceRoots.push(vscode.Uri.file(baseDebugReactUiPath));
		}

		webviewView.webview.options = {
			// Allow scripts in the webview.
			enableScripts: true,

			// And restrict the webview to only loading content from our extension's `webview` directory.
			localResourceRoots
		};

		webviewView.webview.html = this._getHtmlForWebview(webviewView.webview);

		webviewView.webview.onDidReceiveMessage(
			message => this._handleMessage(message)
		);

		webviewView.onDidChangeVisibility(() => {
			if (!webviewView.visible && this._editedModId) {
				vscode.window.showInformationMessage(
					'The Windhawk sidebar was closed, perhaps accidentally. ' +
					'Restore sidebar? You can also restore it with Ctrl+B.',
					'Restore sidebar'
				).then(value => {
					if (value === 'Restore sidebar') {
						webviewView.show();
					}
				});
			}
		});
	}

	private _getHtmlForWebview(webview: vscode.Webview) {
		const webviewPathOnDisk = baseDebugReactUiPath
			? vscode.Uri.file(baseDebugReactUiPath)
			: vscode.Uri.joinPath(this._extensionUri, 'webview');

		const baseWebviewUri = webview.asWebviewUri(webviewPathOnDisk);
		let html = fs.readFileSync(vscode.Uri.joinPath(webviewPathOnDisk, 'index.html').fsPath, 'utf8');

		const csp = [
			`default-src 'none'`,
			`style-src 'unsafe-inline' ${webview.cspSource}`,
			`img-src ${webview.cspSource} data:`,
			`script-src ${webview.cspSource} blob:`,
			`connect-src ${webview.cspSource}`,
			`font-src ${webview.cspSource}`
		];

		html = html.replace('<head>', `<head>
			<base href="${baseWebviewUri.toString()}/">
			<meta http-equiv="Content-Security-Policy" content="${csp.join('; ')};">
		`);

		html = html.replace(/<body([^>]*)>/, '<body data-content="sidebar"$1>');

		return html;
	}

	public fileWasModified(doc: vscode.TextDocument) {
		const modSourcePath = this._utils.editorWorkspace.getModSourcePath();
		if (doc.uri.toString(true) !== vscode.Uri.file(modSourcePath).toString(true)) {
			return;
		}

		this._editedModModifiedCounter++;

		if (!this._editedModWasModified || this._editedModCompilationFailed) {
			this._editedModWasModified = true;
			this._editedModCompilationFailed = false;
			this._utils.editorWorkspace.markEditorModeModAsModified(true);
			webviewIPC.editedModWasModified(this._view?.webview);
		}
	}

	public compileMod() {
		this._view?.show(true);
		webviewIPC.compileEditedModStart(this._view?.webview);
	}

	private _postEditedModDetails() {
		if (this._editedModId) {
			const localModId = 'local@' + this._editedModId;
			const modConfig = this._utils.modConfig.getModConfig(localModId);
			webviewIPC.setEditedModDetails(this._view?.webview, {
				modId: this._editedModId,
				modDetails: modConfig,
				modWasModified: this._editedModWasModified
			});
		}
	}

	public setEditedMod(modId: string, modWasModified: boolean) {
		this._editedModId = modId;
		this._editedModWasModified = modWasModified;
		this._editedModCompilationFailed = false;
		this._postEditedModDetails();
	}

	public appSettingsUpdated() {
		const newAppSettings = this._utils.appSettings.getAppSettings();
		this._language = newAppSettings.language;

		webviewIPC.setNewAppSettings(this._view?.webview, {
			appUISettings: {
				language: this._language
			}
		});
	}

	private readonly _handleMessageMap: Record<string, (message: any) => void> = {
		getInitialAppSettings: message => {
			try {
				const appSettings = this._utils.appSettings.getAppSettings();
				this._language = appSettings.language;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getInitialAppSettingsReply(this._view?.webview, message.messageId, {
				appUISettings: {
					language: this._language
				}
			});
		},
		getInitialSidebarParams: message => {
			this._postEditedModDetails();
		},
		enableEditedMod: message => {
			const data: EnableEditedModData = message.data;

			let succeeded = false;
			try {
				if (!this._editedModId) {
					throw new Error('No mod is being edited');
				}

				const localModId = 'local@' + this._editedModId;
				this._utils.modConfig.enableMod(localModId, data.enable);

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.enableEditedModReply(this._view?.webview, message.messageId, {
				enabled: data.enable,
				succeeded
			});
		},
		enableEditedModLogging: message => {
			const data: EnableEditedModLoggingData = message.data;

			let succeeded = false;
			try {
				if (!this._editedModId) {
					throw new Error('No mod is being edited');
				}

				const localModId = 'local@' + this._editedModId;
				this._utils.modConfig.enableLogging(localModId, data.enable);

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.enableEditedModLoggingReply(this._view?.webview, message.messageId, {
				enabled: data.enable,
				succeeded
			});
		},
		compileEditedMod: async message => {
			const data: CompileEditedModData = message.data;

			if (this._editedModBeingCompiled) {
				return;
			}

			this._editedModBeingCompiled = true;

			const modifiedCounterStart = this._editedModModifiedCounter;

			let succeeded = false;
			let clearModified = false;

			try {
				windhawkCompilerOutput?.clear();

				if (!this._editedModId) {
					throw new Error('No mod is being edited');
				}

				const oldModId = this._editedModId;
				const localOldModId = 'local@' + this._editedModId;

				const modSourcePath = this._utils.editorWorkspace.getModSourcePath();
				const modSourceUri = vscode.Uri.file(modSourcePath);

				// Get text from open editor if available, otherwise read from disk.
				const openEditor = vscode.window.visibleTextEditors.find(
					editor => editor.document.uri.toString(true) === modSourceUri.toString(true)
				);

				let modSource: string;
				if (openEditor) {
					modSource = openEditor.document.getText();
				} else {
					modSource = fs.readFileSync(modSourcePath, 'utf8');
				}

				const metadata = this._utils.modSource.extractMetadata(modSource, this._language);
				if (!metadata.id) {
					throw new Error('Mod id must be specified in the source code');
				}

				const modId = metadata.id;
				const localModId = 'local@' + modId;

				if (modId !== oldModId) {
					if (this._utils.modSource.doesSourceExist(localModId) || this._utils.modConfig.doesConfigExist(localModId)) {
						throw new Error('Mod id specified in the source code already exists');
					}
				}

				const initialSettings = this._utils.modSource.extractInitialSettingsForEngine(modSource);

				let previousInitialSettings: Record<string, string | number> | undefined;
				try {
					const prev = this._utils.modSource.extractInitialSettingsForEngine(
						this._utils.modSource.getSource(localModId)
					);
					if (prev) {
						previousInitialSettings = prev;
					}
				} catch (e) {
					if (e.code !== 'ENOENT') {
						console.error('Failed to extract previous initial settings for engine:', e);
					}
				}

				const { targetDllName } = await this._utils.compiler.compileMod(
					localModId,
					metadata.version || '',
					metadata.include || [],
					modSource,
					metadata.architecture || [],
					metadata.compilerOptions,
					this._utils.editorWorkspace.getWorkspaceFolder()
				);

				if (modId !== oldModId) {
					this._utils.modConfig.changeModId(localOldModId, localModId);
				}

				this._utils.modConfig.setModConfig(localModId, {
					libraryFileName: targetDllName,
					disabled: data.disabled,
					loggingEnabled: data.loggingEnabled,
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

				this._utils.modSource.setSource(localModId, modSource);

				if (modId !== oldModId) {
					this._utils.modSource.deleteSource(localOldModId);

					this._utils.editorWorkspace.setEditorModeModId(modId);

					this._editedModId = modId;
					webviewIPC.setEditedModId(this._view?.webview, {
						modId
					});
				}

				this._utils.modFiles.deleteOldModFiles(localModId, metadata.architecture || [], targetDllName);

				if (data.loggingEnabled) {
					windhawkLogOutput?.createOrShow(true);
				} else {
					windhawkCompilerOutput?.hide();
				}

				WindhawkPanel.refreshIfExists('Preview', {
					previewModId: localModId
				});

				this._editedModCompilationFailed = false;

				clearModified = (modifiedCounterStart === this._editedModModifiedCounter);
				if (clearModified) {
					this._editedModWasModified = false;
					this._utils.editorWorkspace.markEditorModeModAsModified(false);
				}

				succeeded = true;
			} catch (e) {
				reportCompilerException(e);
				this._editedModCompilationFailed = true;
			}

			webviewIPC.compileEditedModReply(this._view?.webview, message.messageId, {
				succeeded,
				clearModified
			});

			this._editedModBeingCompiled = false;
		},
		stopCompileEditedMod: async message => {
			try {
				if (this._editedModBeingCompiled) {
					this._utils.compiler.cancelCompilation();
				}
			} catch (e) {
				reportException(e);
			}
		},
		previewEditedMod: async message => {
			try {
				if (!this._editedModId) {
					throw new Error('No mod is being edited');
				}

				const localModId = 'local@' + this._editedModId;
				await vscode.commands.executeCommand('windhawk.start', {
					title: 'Preview',
					createColumn: vscode.ViewColumn.Beside,
					params: {
						previewModId: localModId
					}
				});
			} catch (e) {
				reportException(e);
			}
		},
		showLogOutput: message => {
			try {
				windhawkLogOutput?.createOrShow();
			} catch (e) {
				reportException(e);
			}
		},
		exitEditorMode: async message => {
			const data: ExitEditorModeData = message.data;

			let succeeded = false;
			try {
				if (!await vscode.workspace.saveAll(true)) {
					throw new Error('Failed to save all files');
				}

				windhawkLogOutput?.dispose();

				if (this._editedModId) {
					if (this._editedModWasModified && data.saveToDrafts) {
						this._utils.editorWorkspace.saveModToDrafts(this._editedModId);
					} else {
						this._utils.editorWorkspace.deleteModFromDrafts(this._editedModId);
					}
				}

				this._editedModId = undefined;
				this._editedModWasModified = false;
				this._editedModCompilationFailed = false;
				await this._utils.editorWorkspace.exitEditorMode();

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.exitEditorModeReply(this._view?.webview, message.messageId, {
				succeeded
			});
		}
	};

	private _handleMessage(message: any) {
		const { command, ...rest } = message;
		this._handleMessageMap[command](rest);
	}
}

function reportException(e: any) {
	console.error(e);
	vscode.window.showErrorMessage(e.message);
}

function reportCompilerException(e: any, treatCompilationErrorAsException = false) {
	if (!(e instanceof CompilerError)) {
		reportException(e);
		return;
	}

	try {
		let log = '';

		const stdout = e.stdout.trim();
		const stderr = e.stderr.trim();

		if (e.exitCode === null) {
			// Likely aborted, so don't show anything.
		} else if ((stdout === '' && stderr === '') || e.exitCode !== 1) {
			const exitCodeStr = e.exitCode !== null ? `0x${e.exitCode.toString(16)}` : 'unknown';
			log = `Exit code: ${exitCodeStr}\n`;
		}

		if (stdout !== '') {
			if (log !== '') {
				log += '\n';
			}
			log += stdout + '\n';
		}

		if (stderr !== '') {
			if (log !== '') {
				log += '\n';
			}
			log += stderr + '\n';
		}

		windhawkCompilerOutput?.append(log);
		windhawkCompilerOutput?.show();

		if (treatCompilationErrorAsException) {
			reportException(e);
		}
	} catch (e) {
		reportException(e);
	}
}

// https://stackoverflow.com/a/6234804
function escapeHtml(unsafe: string) {
	return unsafe
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;")
		.replace(/'/g, "&#039;");
}
