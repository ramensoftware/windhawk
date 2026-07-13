import * as crypto from 'crypto';
import * as fs from 'fs';
import * as path from 'path';
import * as semver from 'semver';
import * as vscode from 'vscode';
import * as i18n from 'vscode-nls-i18n';
import config from './config';
import {
	AppSettings,
	AppUISettings,
	AsyncOperation,
	Catalog,
	CompilerError,
	CompilerKilled,
	createWindhawkCore,
	InitialSettings,
	InstallModResult,
	ModConfig,
	ModMetadata,
	ParsedModSource,
	WindhawkCore
} from './coreClient';
import { vsCodeLogger } from './extensionLogger';
import { WindhawkLogOutput } from './logOutputChannel';
import EditorWorkspaceUtils from './utils/editorWorkspaceUtils';
import * as webviewIPC from './webviewIPC';
import {
	CompileEditedModData,
	CompileModData,
	CompileModReplyData,
	DeleteModData,
	DevActionReplyData,
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
	SetModSettingsData,
	StartUpdateReplyData,
	UpdateAppSettingsData,
	UpdateInstalledModsDetailsData,
	UpdateModConfigData,
	UpdateModRatingData
} from './webviewIPCMessages';

type AppUtils = {
	core: WindhawkCore,
	editorWorkspace: EditorWorkspaceUtils
};

// Set to a local folder to use a dev environment.
// Set to null to use the 'webview' folder.
const baseDebugReactUiPath: string | null = config.debug.reactProjectBuildPath;

// Raw installed-Windhawk version string; the core session coerces it where
// comparisons are needed.
const rawWindhawkVersion: string | null =
	vscode.extensions.getExtension('m417z.windhawk')?.packageJSON.version ?? null;

const currentWindhawkVersion = semver.coerce(rawWindhawkVersion);

let windhawkLogOutput: WindhawkLogOutput | null = null;
let windhawkCompilerOutput: vscode.OutputChannel | null = null;

export function activate(context: vscode.ExtensionContext) {
	if (!config.debug.disableEnvVarCheck && !process.env['WINDHAWK_UI_PATH']) {
		vscode.window.showErrorMessage('Windhawk: Unsupported environment, perhaps VSCode was launched directly');
		return;
	}

	try {
		i18n.init(context.extensionPath);

		windhawkLogOutput = new WindhawkLogOutput(path.join(context.extensionPath, 'files', 'DbgViewMini.exe'));
		windhawkCompilerOutput = vscode.window.createOutputChannel('Windhawk Compiler');

		const arm64Enabled = process.env['WINDHAWK_ARM64_ENABLED'] === '1';

		// vscode.env.appRoot returns <vscode_dir>\resources\app; the Windhawk app root
		// is three levels up. Overridable for extension development via config.debug.
		const appRoot = config.debug.appRootPath
			?? path.dirname(path.dirname(path.dirname(vscode.env.appRoot)));
		const core = createWindhawkCore({
			appRoot,
			arm64Enabled,
			windhawkVersion: rawWindhawkVersion,
			userAgentProduct: `Windhawk/${currentWindhawkVersion?.version || 'unknown'}`,
			logger: vsCodeLogger,
		});
		const utils: AppUtils = {
			core,
			editorWorkspace: new EditorWorkspaceUtils(),
		};

		const sidebarWebviewViewProvider = new WindhawkViewProvider(context.extensionUri, context.extensionPath, utils);

		context.subscriptions.push(
			vscode.window.registerWebviewViewProvider(WindhawkViewProvider.viewType, sidebarWebviewViewProvider, {
				webviewOptions: {
					retainContextWhenHidden: true
				}
			})
		);

		context.subscriptions.push(
			vscode.workspace.onDidChangeTextDocument(({ contentChanges, document }) => {
				if (contentChanges.length > 0) {
					sidebarWebviewViewProvider.fileWasModified(document);
				}
			})
		);

		const onEnterEditorMode = (modId: string, modWasModified = false) => {
			return sidebarWebviewViewProvider.setEditedMod(modId, modWasModified);
		};

		const onAppSettingsUpdated = () => {
			return sidebarWebviewViewProvider.appSettingsUpdated();
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

		utils.editorWorkspace.restoreEditorMode().then(async ({ modId, modWasModified }) => {
			if (modId) {
				await sidebarWebviewViewProvider.setEditedMod(modId, !!modWasModified);
			}
		}).catch(e => reportException(e));

		utils.core.getProfileWatchInfo().then(({ filePath }) => {
			const onUserProfileModified = async () => {
				const { mtimeMs } = fs.statSync(filePath);
				const { lastModifiedByUserMtimeMs } = await utils.core.getProfileWatchInfo();
				if (mtimeMs !== lastModifiedByUserMtimeMs) {
					WindhawkPanel.userProfileChanged();
				}
			};

			const userProfileWatcher = vscode.workspace.createFileSystemWatcher(
				new vscode.RelativePattern(vscode.Uri.file(filePath), '*'));
			userProfileWatcher.onDidCreate(onUserProfileModified);
			userProfileWatcher.onDidChange(onUserProfileModified);
			context.subscriptions.push(userProfileWatcher);
		}).catch(e => reportException(e));
	} catch (e) {
		reportException(e);
	}
}

type WindhawkPanelCallbacks = {
	onEnterEditorMode: (modId: string, modWasModified: boolean) => Promise<void>,
	onAppSettingsUpdated: () => Promise<void>
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
	private _disposed = false;
	private _language = 'en';
	private _checkForUpdates = true;
	private _alwaysCompileModsLocally = false;
	private _currentUpdateOp: AsyncOperation<void> | null = null;

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
				localResourceRoots,

				// Retain the webview content when the panel is hidden.
				retainContextWhenHidden: true
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
		this._disposed = true;
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

	private get _webview(): vscode.Webview | undefined {
		return this._disposed ? undefined : this._panel.webview;
	}

	private _getHtmlForWebview(webview: vscode.Webview, params?: WindhawkPanelParams) {
		const csp = [
			`default-src 'none'`,
			`style-src 'unsafe-inline' ${webview.cspSource}`,
			`img-src ${webview.cspSource} https://i.imgur.com https://raw.githubusercontent.com https://mods.windhawk.net`,
			`script-src ${webview.cspSource}`,
			`worker-src ${webview.cspSource} blob:`, // For Monaco
			`connect-src ${webview.cspSource} https://mods.windhawk.net https://ramensoftware.com`,
			`font-src ${webview.cspSource}`
		];

		return getHtmlForWebview(webview, this._extensionUri, csp, 'panel', params);
	}

	private async _getAppUISettings(appSettings: AppSettings): Promise<AppUISettings> {
		let updateIsAvailable = false;
		let updateIsAvailableBleedingEdge = false;
		if (!appSettings.disableUpdateCheck) {
			try {
				const updateStatus = await this._utils.core.getAppUpdateStatus();
				updateIsAvailable = updateStatus.updateAvailable;
				updateIsAvailableBleedingEdge = updateStatus.updateAvailableBleedingEdge;
			} catch (e) {
				reportException(e);
			}
		}

		return {
			language: appSettings.language,
			devModeOptOut: appSettings.devModeOptOut,
			loggingEnabled: (
				appSettings.loggingVerbosity > 0 ||
				appSettings.engine.loggingVerbosity > 0
			),
			updateIsAvailable,
			updateIsAvailableBleedingEdge,
			safeMode: appSettings.safeMode
		};
	}

	private async _userProfileChanged() {
		try {
			// First, recalculate UI settings, since the update availability value
			// depends on the user profile.
			const appSettings = await this._utils.core.getAppSettings();
			this._language = appSettings.language;
			this._checkForUpdates = !appSettings.disableUpdateCheck;
			this._alwaysCompileModsLocally = appSettings.alwaysCompileModsLocally;

			webviewIPC.setNewAppSettings(this._webview, {
				appUISettings: await this._getAppUISettings(appSettings)
			});

			// Next, recalculate mod values which depend on the user profile.
			// No profile sync here: this runs in reaction to a profile change,
			// and the previous implementation performed no writes either.
			const { mods, loadErrors } = await this._utils.core.listInstalledMods({
				language: this._language,
				checkForUpdates: this._checkForUpdates,
				syncProfile: false,
			});
			for (const { modId, error } of loadErrors) {
				vscode.window.showErrorMessage(`Failed to load mod ${modId}: ${error}`);
			}

			const details: UpdateInstalledModsDetailsData['details'] = {};
			for (const [modId, entry] of Object.entries(mods)) {
				details[modId] = {
					updateAvailable: entry.updateAvailable,
					userRating: entry.userRating
				};
			}

			webviewIPC.updateInstalledModsDetails(this._webview, {
				details
			});
		} catch (e) {
			reportException(e);
		}
	}

	private readonly _handleMessageMap: Record<string, (message: any) => void> = {
		getInitialAppSettings: async message => {
			let appUISettings: Partial<AppUISettings> = {};
			try {
				const appSettings = await this._utils.core.getAppSettings();
				this._language = appSettings.language;
				this._checkForUpdates = !appSettings.disableUpdateCheck;
				this._alwaysCompileModsLocally = appSettings.alwaysCompileModsLocally;

				appUISettings = await this._getAppUISettings(appSettings);
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getInitialAppSettingsReply(this._webview, message.messageId, {
				appUISettings
			});
		},
		getInstalledMods: async message => {
			const installedMods: GetInstalledModsReplyData['installedMods'] = {};
			try {
				const { mods, loadErrors } = await this._utils.core.listInstalledMods({
					language: this._language,
					checkForUpdates: this._checkForUpdates,
					syncProfile: true,
				});
				for (const { modId, error } of loadErrors) {
					vscode.window.showErrorMessage(`Failed to load mod ${modId}: ${error}`);
				}
				Object.assign(installedMods, mods);
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getInstalledModsReply(this._webview, message.messageId, {
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

			webviewIPC.getFeaturedModsReply(this._webview, message.messageId, {
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

				// Pure read: decorating the repository listing with installed
				// state must not write the profile.
				const { mods: installedMods, loadErrors } = await this._utils.core.listInstalledMods({
					language: this._language,
					checkForUpdates: this._checkForUpdates,
					syncProfile: false,
				});
				for (const { modId, error } of loadErrors) {
					vscode.window.showErrorMessage(`Failed to load mod ${modId}: ${error}`);
				}

				for (const [modId, entry] of Object.entries(installedMods)) {
					if (mods[modId]) {
						mods[modId].installed = {
							metadata: entry.metadata,
							config: entry.config,
							userRating: entry.userRating
						};
					}
				}
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getRepositoryModsReply(this._webview, message.messageId, {
				mods
			});
		},
		getModSourceData: async message => {
			const data: GetModSourceDataData = message.data;

			let source: string | null = null;
			try {
				source = await this._utils.core.getModSource(data.modId);
			} catch (e) {
				reportException(e);
			}

			let metadata: ModMetadata | null = null;
			let readme: string | null = null;
			let initialSettings: InitialSettings | null = null;
			if (source) {
				const parsed = await this._utils.core.parseModSource(source, this._language);
				reportModSourceParseErrors(parsed);
				({ metadata, readme, initialSettings } = parsed);
			}

			webviewIPC.getModSourceDataReply(this._webview, message.messageId, {
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

			let source: string | null = null;
			try {
				// CRLF normalization happens inside the core.
				source = await this._utils.core.fetchRepoModSource(data.modId, data.version);
			} catch (e) {
				reportException(e);
			}

			let metadata: ModMetadata | null = null;
			let readme: string | null = null;
			let initialSettings: InitialSettings | null = null;
			if (source) {
				const parsed = await this._utils.core.parseModSource(source, this._language);
				reportModSourceParseErrors(parsed);
				({ metadata, readme, initialSettings } = parsed);
			}

			webviewIPC.getRepositoryModSourceDataReply(this._webview, message.messageId, {
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

			let versions: GetModVersionsReplyData['versions'] = [];
			try {
				versions = await this._utils.core.fetchModVersions(modId);
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getModVersionsReply(this._webview, message.messageId, {
				modId,
				versions
			});
		},
		getModSettings: async message => {
			const data: GetModSettingsData = message.data;

			let settings: Record<string, any> = {};
			try {
				settings = await this._utils.core.getModSettings(data.modId);
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getModSettingsReply(this._webview, message.messageId, {
				modId: data.modId,
				settings
			});
		},
		setModSettings: async message => {
			const data: SetModSettingsData = message.data;

			let succeeded = false;
			try {
				await this._utils.core.setModSettings(data.modId, data.settings);

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.setModSettingsReply(this._webview, message.messageId, {
				modId: data.modId,
				succeeded
			});
		},
		getModConfig: async message => {
			const data: GetModConfigData = message.data;

			let config: ModConfig | null = null;
			try {
				config = await this._utils.core.getModConfig(data.modId);
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getModConfigReply(this._webview, message.messageId, {
				modId: data.modId,
				config
			});
		},
		updateModConfig: async message => {
			const data: UpdateModConfigData = message.data;

			let succeeded = false;
			try {
				await this._utils.core.updateModConfig(data.modId, data.config);

				webviewIPC.setNewModConfig(this._webview, {
					modId: data.modId,
					config: data.config
				});

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.updateModConfigReply(this._webview, message.messageId, {
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

				const metadata = await extractMetadataOrThrow(this._utils.core, modSource, this._language);
				if (!metadata.id) {
					throw new Error('Mod id must be specified in the source code');
				} else if (metadata.id !== modId) {
					throw new Error('Mod id specified in the source code doesn\'t match');
				}

				const result = await this._utils.core.installMod({
					storageId: modId,
					source: modSource,
					metadata,
					disabled: data.disabled,
					loggingEnabled: data.loggingEnabled,
					compileLocally: this._alwaysCompileModsLocally,
					trackInProfile: true,
				}).result;

				installedModDetails = {
					metadata,
					config: result.config
				};
			} catch (e) {
				reportCompilerException(e, true);
			}

			webviewIPC.installModReply(this._webview, message.messageId, {
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
				const modSource = await this._utils.core.getModSource(modId);

				const metadata = await extractMetadataOrThrow(this._utils.core, modSource, this._language);
				if (!metadata.id) {
					throw new Error('Mod id must be specified in the source code');
				} else if (metadata.id !== modId.replace(/^local@/, '')) {
					throw new Error('Mod id specified in the source code doesn\'t match');
				}

				const result = await this._utils.core.compileInstalledMod({
					storageId: modId,
					source: modSource,
					metadata,
				}).result;

				compiledModDetails = {
					metadata,
					config: result.config
				};
			} catch (e) {
				reportCompilerException(e, true);
			}

			webviewIPC.compileModReply(this._webview, message.messageId, {
				modId: data.modId,
				compiledModDetails
			});
		},
		enableMod: async message => {
			const data: EnableModData = message.data;

			let succeeded = false;
			try {
				const modId: string = data.modId;
				const enable: boolean = data.enable;

				await this._utils.core.setModEnabled(modId, enable);

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.enableModReply(this._webview, message.messageId, {
				modId: data.modId,
				enabled: data.enable,
				succeeded
			});
		},
		createNewMod: async message => {
			let reply: DevActionReplyData = {};
			try {
				const modSourcePath = path.join(this._extensionPath, 'files', 'mod_template.wh.cpp');
				let modSource = fs.readFileSync(modSourcePath, 'utf8');

				const metadata = await extractMetadataOrThrow(this._utils.core, modSource, this._language);
				if (!metadata.id) {
					throw new Error('Mod id must be specified in the source code');
				}

				let newModId = metadata.id;
				let localModId = 'local@' + newModId;
				if (await this._utils.core.doesModExist(localModId)) {
					let counter = 2;
					let modIdSuffix;
					for (; ;) {
						modIdSuffix = '-' + counter;
						newModId = metadata.id + modIdSuffix;
						localModId = 'local@' + newModId;

						const exists = await this._utils.core.doesModExist(localModId);
						if (!exists) {
							break;
						}

						counter++;
					}

					const modNameSuffix = ` (${counter})`;
					modSource = await this._utils.core.appendToModIdAndName(modSource, modIdSuffix, modNameSuffix);
				}

				this._utils.editorWorkspace.initializeFromModSource(modSource);

				await this._callbacks.onEnterEditorMode(newModId, false);

				await this._utils.editorWorkspace.enterEditorMode(newModId);
			} catch (e) {
				reportException(e);
				reply = { error: { code: 'INTERNAL', message: e instanceof Error ? e.message : String(e) } };
			}

			webviewIPC.createNewModReply(this._webview, message.messageId, reply);
		},
		editMod: async message => {
			const data: EditModData = message.data;

			let reply: DevActionReplyData = {};
			try {
				const modSource = await this._utils.core.getModSource(data.modId);

				const metadata = await extractMetadataOrThrow(this._utils.core, modSource, this._language);
				if (!metadata.id) {
					throw new Error('Mod id must be specified in the source code');
				}

				const modSourceFromDrafts = this._utils.editorWorkspace.loadModFromDrafts(metadata.id);
				if (modSourceFromDrafts) {
					this._utils.editorWorkspace.deleteModFromDrafts(metadata.id);
				}

				this._utils.editorWorkspace.initializeFromModSource(modSource, modSourceFromDrafts);

				await this._callbacks.onEnterEditorMode(metadata.id, !!modSourceFromDrafts);

				await this._utils.editorWorkspace.enterEditorMode(metadata.id, !!modSourceFromDrafts);
			} catch (e) {
				reportException(e);
				reply = { error: { code: 'INTERNAL', message: e instanceof Error ? e.message : String(e) } };
			}

			webviewIPC.editModReply(this._webview, message.messageId, reply);
		},
		forkMod: async message => {
			const data: ForkModData = message.data;

			let reply: DevActionReplyData = {};
			try {
				let modSource = data.modSource || await this._utils.core.getModSource(data.modId);

				const metadata = await extractMetadataOrThrow(this._utils.core, modSource, this._language);
				if (!metadata.id) {
					throw new Error('Mod id must be specified in the source code');
				} else if (metadata.id !== data.modId.replace(/^local@/, '')) {
					throw new Error('Mod id specified in the source code doesn\'t match');
				}

				let modIdSuffix = '-fork';
				let forkModId = metadata.id + modIdSuffix;
				let localModId = 'local@' + forkModId;
				let modNameSuffix = ' - Fork';
				if (await this._utils.core.doesModExist(localModId)) {
					let counter = 2;
					for (; ;) {
						modIdSuffix = '-fork' + counter;
						forkModId = metadata.id + modIdSuffix;
						localModId = 'local@' + forkModId;

						const exists = await this._utils.core.doesModExist(localModId);
						if (!exists) {
							break;
						}

						counter++;
					}

					modNameSuffix = ` - Fork (${counter})`;
				}

				modSource = await this._utils.core.appendToModIdAndName(modSource, modIdSuffix, modNameSuffix);

				this._utils.editorWorkspace.initializeFromModSource(modSource);

				await this._callbacks.onEnterEditorMode(forkModId, false);

				await this._utils.editorWorkspace.enterEditorMode(forkModId);
			} catch (e) {
				reportException(e);
				reply = { error: { code: 'INTERNAL', message: e instanceof Error ? e.message : String(e) } };
			}

			webviewIPC.forkModReply(this._webview, message.messageId, reply);
		},
		deleteMod: async message => {
			const data: DeleteModData = message.data;

			let succeeded = false;
			try {
				const modId: string = data.modId;

				await this._utils.core.removeMod(modId);

				if (modId.startsWith('local@')) {
					this._utils.editorWorkspace.deleteModFromDrafts(modId.replace(/^local@/, ''));
				}

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.deleteModReply(this._webview, message.messageId, {
				modId: data.modId,
				succeeded
			});
		},
		updateModRating: async message => {
			const data: UpdateModRatingData = message.data;

			let succeeded = false;

			try {
				await this._utils.core.setModRating(data.modId, data.rating);

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.updateModRatingReply(this._webview, message.messageId, {
				modId: data.modId,
				rating: data.rating,
				succeeded
			});
		},
		getAppSettings: async message => {
			let appSettings: Partial<AppSettings> = {};
			try {
				appSettings = await this._utils.core.getAppSettings();
			} catch (e) {
				reportException(e);
			}

			webviewIPC.getAppSettingsReply(this._webview, message.messageId, {
				appSettings
			});
		},
		updateAppSettings: async message => {
			const data: UpdateAppSettingsData = message.data;

			let succeeded = false;
			try {
				const appSettings: Partial<AppSettings> = data.appSettings;

				const { requiresRestart, requiresNotify } = await this._utils.core.applyAppSettings(appSettings);

				const newAppSettings = await this._utils.core.getAppSettings();
				this._language = newAppSettings.language;
				this._checkForUpdates = !newAppSettings.disableUpdateCheck;
				this._alwaysCompileModsLocally = newAppSettings.alwaysCompileModsLocally;

				webviewIPC.setNewAppSettings(this._webview, {
					appUISettings: await this._getAppUISettings(newAppSettings)
				});

				await this._callbacks.onAppSettingsUpdated();

				if (requiresRestart) {
					await this._utils.core.notifyTray('restartBg');
					vscode.window.showInformationMessage('Windhawk was restarted');
				} else if (requiresNotify) {
					await this._utils.core.notifyTray('appSettingsChanged');
				}

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.updateAppSettingsReply(this._webview, message.messageId, {
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
			let result: StartUpdateReplyData = { succeeded: true };

			const updateOp = this._utils.core.startUpdate({
				onProgress: (data) => {
					webviewIPC.updateDownloadProgress(this._webview, data);
				},
				onInstalling: () => {
					webviewIPC.updateInstalling(this._webview, {});
				}
			});
			this._currentUpdateOp = updateOp;

			try {
				await updateOp.result;
			} catch (e) {
				reportException(e);
				result = {
					succeeded: false,
					error: e instanceof Error ? e.message : String(e),
				};
			}

			webviewIPC.startUpdateReply(this._webview, message.messageId, result);
		},
		cancelUpdate: message => {
			let succeeded = false;
			try {
				// cancel() of a finished (or never-started) update is a
				// harmless no-op returning false, like the previous
				// cancelUpdate service call.
				if (this._currentUpdateOp?.cancel()) {
					succeeded = true;
				}
			} catch (e) {
				reportException(e);
			}

			webviewIPC.cancelUpdateReply(this._webview, message.messageId, {
				succeeded
			});
		}
	};

	private _handleMessage(message: any) {
		const { command, ...rest } = message;
		this._handleMessageMap[command](rest);
	}

	private async _fetchRepositoryMods(language: string) {
		const catalog = await this._utils.core.fetchCatalog(language);
		await this._updateUserProfileJson(catalog);
		return catalog.mods;
	}

	private async _updateUserProfileJson(catalog: Catalog) {
		const { profileUpdated } = await this._utils.core.syncCatalogToProfile(catalog);

		if (profileUpdated && this._checkForUpdates) {
			await this._utils.core.notifyTray('newUpdatesFound');
		}
	}
}

class WindhawkViewProvider implements vscode.WebviewViewProvider {
	public static readonly viewType = 'windhawk.sidebar';

	private _view?: vscode.WebviewView;
	private readonly _extensionUri: vscode.Uri;
	private readonly _extensionPath: string;
	private readonly _utils: AppUtils;
	private _disposables: vscode.Disposable[] = [];
	private _language = 'en';
	private _editedModId?: string;
	private _editedModWasModified = false;
	private _editedModModifiedCounter = 0;
	private _editedModBeingCompiled = false;
	private _editedModCompilationFailed = false;
	private _currentCompileOp: AsyncOperation<InstallModResult> | null = null;

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

		// Listen for when the view is disposed.
		// This happens when the user closes the view or when the view is closed programmatically.
		webviewView.onDidDispose(() => this.dispose(), null, this._disposables);

		webviewView.webview.onDidReceiveMessage(
			message => this._handleMessage(message),
			null,
			this._disposables
		);

		if (process.env['WINDHAWK_UI_EDITOR_NO_SIDEBAR_CLOSE_WARNING'] !== '1') {
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
			}, null, this._disposables);
		}
	}

	public dispose() {
		this._view = undefined;

		while (this._disposables.length) {
			const x = this._disposables.pop();
			if (x) {
				x.dispose();
			}
		}
	}

	private _getHtmlForWebview(webview: vscode.Webview) {
		const csp = [
			`default-src 'none'`,
			`style-src 'unsafe-inline' ${webview.cspSource}`,
			`img-src ${webview.cspSource}`,
			`script-src ${webview.cspSource}`,
			`connect-src ${webview.cspSource}`,
			`font-src ${webview.cspSource}`
		];

		return getHtmlForWebview(webview, this._extensionUri, csp, 'sidebar');
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
		if (!this._view?.visible) {
			this._view?.show(true);
		} else {
			webviewIPC.compileEditedModStart(this._view?.webview);
		}
	}

	private async _postEditedModDetails() {
		if (this._editedModId) {
			const localModId = 'local@' + this._editedModId;
			const modConfig = await this._utils.core.getModConfig(localModId);
			const vscodeConfig = vscode.workspace.getConfiguration();
			webviewIPC.setEditedModDetails(this._view?.webview, {
				modId: this._editedModId,
				modDetails: modConfig,
				modWasModified: this._editedModWasModified,
				noWindhawkExitButton: !!vscodeConfig.get('windhawk.noWindhawkExitButton')
			});
		}
	}

	public async setEditedMod(modId: string, modWasModified: boolean) {
		this._editedModId = modId;
		this._editedModWasModified = modWasModified;
		this._editedModCompilationFailed = false;
		await this._postEditedModDetails();
	}

	public async appSettingsUpdated() {
		const newAppSettings = await this._utils.core.getAppSettings();
		this._language = newAppSettings.language;

		webviewIPC.setNewAppSettings(this._view?.webview, {
			appUISettings: {
				language: this._language
			}
		});
	}

	private readonly _handleMessageMap: Record<string, (message: any) => void> = {
		getInitialAppSettings: async message => {
			try {
				const appSettings = await this._utils.core.getAppSettings();
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
		getInitialSidebarParams: async message => {
			// The catch belongs here, not in _postEditedModDetails: the other
			// caller (setEditedMod) must keep propagating failures so the
			// enter-editor-mode flows abort like they always did.
			try {
				await this._postEditedModDetails();
			} catch (e) {
				reportException(e);
			}
		},
		enableEditedMod: async message => {
			const data: EnableEditedModData = message.data;

			let succeeded = false;
			try {
				if (!this._editedModId) {
					throw new Error('No mod is being edited');
				}

				// The edited mod is always local@, for which setModEnabled is
				// exactly the raw config write (no profile bookkeeping).
				const localModId = 'local@' + this._editedModId;
				await this._utils.core.setModEnabled(localModId, data.enable);

				succeeded = true;
			} catch (e) {
				reportException(e);
			}

			webviewIPC.enableEditedModReply(this._view?.webview, message.messageId, {
				enabled: data.enable,
				succeeded
			});
		},
		enableEditedModLogging: async message => {
			const data: EnableEditedModLoggingData = message.data;

			let succeeded = false;
			try {
				if (!this._editedModId) {
					throw new Error('No mod is being edited');
				}

				const localModId = 'local@' + this._editedModId;
				await this._utils.core.setModLoggingEnabled(localModId, data.enable);

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

				const metadata = await extractMetadataOrThrow(this._utils.core, modSource, this._language);
				if (!metadata.id) {
					throw new Error('Mod id must be specified in the source code');
				}

				const modId = metadata.id;
				const localModId = 'local@' + modId;

				if (modId !== oldModId) {
					if (await this._utils.core.doesModExist(localModId)) {
						throw new Error('Mod id specified in the source code already exists');
					}
				}

				const compileOp = this._utils.core.installMod({
					storageId: localModId,
					source: modSource,
					metadata,
					disabled: data.disabled,
					loggingEnabled: data.loggingEnabled,
					compileLocally: true,
					trackInProfile: false,
					pchFolder: this._utils.editorWorkspace.getWorkspaceFolder(),
					renameFromStorageId: modId !== oldModId ? localOldModId : undefined,
				});
				this._currentCompileOp = compileOp;
				await compileOp.result;

				if (modId !== oldModId) {
					this._utils.editorWorkspace.setEditorModeModId(modId);

					this._editedModId = modId;
					webviewIPC.setEditedModId(this._view?.webview, {
						modId
					});
				}

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
					this._currentCompileOp?.cancel();
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

// Surface each failed parseModSource section the way the inline extract
// calls used to: one error notification per section, parsing of the other
// sections unaffected.
function reportModSourceParseErrors(parsed: ParsedModSource) {
	for (const error of [parsed.errors.metadata, parsed.errors.readme, parsed.errors.initialSettings]) {
		if (error !== undefined) {
			reportException(new Error(error));
		}
	}
}

// Metadata-or-throw convenience over parseModSource for the handlers that
// previously called modSource.extractMetadata directly (which threw on any
// parse failure).
async function extractMetadataOrThrow(core: WindhawkCore, modSource: string, language: string): Promise<ModMetadata> {
	const parsed = await core.parseModSource(modSource, language);
	if (!parsed.metadata) {
		throw new Error(parsed.errors.metadata ?? 'Failed to parse mod metadata');
	}
	return parsed.metadata;
}

function reportCompilerException(e: any, treatCompilationErrorAsException = false) {
	if (e instanceof CompilerKilled) {
		windhawkCompilerOutput?.append(e.message + '\n');
		windhawkCompilerOutput?.show();
		return;
	}

	if (!(e instanceof CompilerError)) {
		reportException(e);
		return;
	}

	try {
		let log = '';

		const stdout = e.stdout.trim();
		const stderr = e.stderr.trim();

		if ((stdout === '' && stderr === '') || e.exitCode !== 1) {
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

function getHtmlForWebview(
	webview: vscode.Webview,
	extensionUri: vscode.Uri,
	cspRules: string[],
	bodyDataContent: string,
	bodyDataParams?: any
): string {
	const nonce = crypto.randomBytes(16).toString('hex');

	const cspRulesWithNonce = cspRules.map(rule =>
		rule.startsWith('script-src ') ? rule + ` 'nonce-${nonce}'` : rule
	);

	const webviewPathOnDisk = baseDebugReactUiPath
		? vscode.Uri.file(baseDebugReactUiPath)
		: vscode.Uri.joinPath(extensionUri, 'webview');

	const baseWebviewUri = webview.asWebviewUri(webviewPathOnDisk);
	let html = fs.readFileSync(vscode.Uri.joinPath(webviewPathOnDisk, 'index.html').fsPath, 'utf8');

	html = html.replace('<head>', `<head>
		<base href="${baseWebviewUri.toString()}/">
		<meta http-equiv="Content-Security-Policy" content="${cspRulesWithNonce.join('; ')};">
		<script nonce="${nonce}">(() => {
			let lastFocused = null;
			document.addEventListener('focusin', (e) => { lastFocused = e.target; });
			document.addEventListener('focusout', () => {
				setTimeout(() => {
					if (document.hasFocus() && (!document.activeElement || document.activeElement === document.body)) {
						lastFocused = null;
					}
				}, 0);
			});
			window.addEventListener('focus', () => {
				setTimeout(() => {
					if (lastFocused && (!document.activeElement || document.activeElement === document.body)) {
						lastFocused.focus();
					}
				}, 0);
			});
		})();</script>
	`);

	const dataParams = bodyDataParams ? ` data-params="${escapeHtml(JSON.stringify(bodyDataParams))}"` : '';
	const dataVscodeContext = ` data-vscode-context='{"preventDefaultContextMenuItems": true}'`;

	html = html.replace(/<body([^>]*)>/, `<body data-content="${bodyDataContent}"${dataParams}${dataVscodeContext}$1>`);

	return html;
}
