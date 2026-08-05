import * as crypto from 'crypto';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import * as i18n from 'vscode-nls-i18n';
import config from './config';
import {
	AppSettings,
	AppUISettings,
	AsyncOperation,
	CompileInstalledModResult,
	CompilerError,
	CompilerKilled,
	CoreDllError,
	createWindhawkCore,
	ImportUserDataResult,
	InitialSettings,
	InstallModResult,
	MAX_ARCHIVE_BYTES,
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
	WEBVIEW_IPC_CONTRACT_VERSION,
	CancelCompileModData,
	CancelInstallModData,
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
	ExportUserDataData,
	ExportUserDataReplyData,
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
	ImportUserDataData,
	ImportUserDataReplyData,
	InspectUserDataData,
	InspectUserDataReplyData,
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
// comparisons are needed and builds the repository User-Agent from it.
const rawWindhawkVersion: string | null =
	vscode.extensions.getExtension('m417z.windhawk')?.packageJSON.version ?? null;

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

		// vscode.env.appRoot returns <vscode_dir>\resources\app; the Windhawk app root
		// is three levels up. Overridable for extension development via config.debug.
		const appRoot = config.debug.appRootPath
			?? path.dirname(path.dirname(path.dirname(vscode.env.appRoot)));
		const core = createWindhawkCore({
			appRoot,
			windhawkVersion: rawWindhawkVersion,
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
	private _currentImportOp: AsyncOperation<ImportUserDataResult> | null = null;
	// The in-flight installMod / compileMod operations, keyed by the mod each was
	// started for. Unlike the update and the import there can be several at once -
	// one per mod card - so a cancel names the mod it means and these are maps, not
	// single slots. An entry lives only while its operation runs.
	private _currentInstallOps = new Map<string, AsyncOperation<InstallModResult>>();
	private _currentCompileOps = new Map<string, AsyncOperation<CompileInstalledModResult>>();

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

	// Re-read the app settings and announce them: refresh the values cached off them,
	// push the recomputed appUISettings to the webview (the language it translates
	// through, and the app-level indicators), and let the sidebar refresh. Called
	// after this panel's own write (updateAppSettings), and for a write it did not
	// drive - a user-data import applies the archive's app settings inside the core,
	// after which the app would otherwise keep showing the old ones until a reload.
	//
	// The tray is not poked here: which action a change calls for is the caller's to
	// decide (updateAppSettings from the intents it was handed, the import from its
	// own summary).
	private async _announceAppSettings() {
		const appSettings = await this._utils.core.getAppSettings();
		this._language = appSettings.language;
		this._checkForUpdates = !appSettings.disableUpdateCheck;
		this._alwaysCompileModsLocally = appSettings.alwaysCompileModsLocally;

		webviewIPC.setNewAppSettings(this._webview, {
			appUISettings: await this._getAppUISettings(appSettings)
		});

		await this._callbacks.onAppSettingsUpdated();
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
				contractVersion: WEBVIEW_IPC_CONTRACT_VERSION,
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

				const op = this._utils.core.installMod({
					storageId: modId,
					source: modSource,
					metadata,
					disabled: data.disabled,
					loggingEnabled: data.loggingEnabled,
					compileLocally: this._alwaysCompileModsLocally,
					trackInProfile: true,
				});

				// Track the operation for the duration of the install so
				// cancelInstallMod can find it, and drop it however it settles -
				// a completion, a failure, or the cancel itself.
				this._currentInstallOps.set(modId, op);
				let result: InstallModResult;
				try {
					result = await op.result;
				} finally {
					this._currentInstallOps.delete(modId);
				}

				// A successful compile may still have emitted warnings; reveal them
				// without stealing focus.
				if (appendCompilerWarnings(result.warnings)) {
					windhawkCompilerOutput?.show(true);
				}

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
		cancelInstallMod: message => {
			const data: CancelInstallModData = message.data;

			let succeeded = false;
			try {
				// cancel() of a finished (or never-started) install is a harmless
				// no-op returning false, like cancelUpdate. The installMod reply
				// still arrives, from the operation's own rejection.
				if (this._currentInstallOps.get(data.modId)?.cancel()) {
					succeeded = true;
				}
			} catch (e) {
				reportException(e);
			}

			webviewIPC.cancelInstallModReply(this._webview, message.messageId, {
				modId: data.modId,
				succeeded
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

				const op = this._utils.core.compileInstalledMod({
					storageId: modId,
					source: modSource,
					metadata,
				});

				// Tracked for cancelCompileMod exactly as the install is, keyed by
				// the storage id the request named.
				this._currentCompileOps.set(modId, op);
				let result: CompileInstalledModResult;
				try {
					result = await op.result;
				} finally {
					this._currentCompileOps.delete(modId);
				}

				// A successful recompile may still have emitted warnings; reveal
				// them without stealing focus.
				if (appendCompilerWarnings(result.warnings)) {
					windhawkCompilerOutput?.show(true);
				}

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
		cancelCompileMod: message => {
			const data: CancelCompileModData = message.data;

			let succeeded = false;
			try {
				// The recompile twin of cancelInstallMod; see there.
				if (this._currentCompileOps.get(data.modId)?.cancel()) {
					succeeded = true;
				}
			} catch (e) {
				reportException(e);
			}

			webviewIPC.cancelCompileModReply(this._webview, message.messageId, {
				modId: data.modId,
				succeeded
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

				const compileFlags = await this._utils.core.getCompileFlags();
				this._utils.editorWorkspace.initializeFromModSource(modSource, compileFlags);

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

				const compileFlags = await this._utils.core.getCompileFlags();
				this._utils.editorWorkspace.initializeFromModSource(modSource, compileFlags, modSourceFromDrafts);

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

				const compileFlags = await this._utils.core.getCompileFlags();
				this._utils.editorWorkspace.initializeFromModSource(modSource, compileFlags);

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

				await this._announceAppSettings();

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
				// A user-initiated cancel rejects with CANCELED: an acknowledged
				// outcome the update dialog already handled, not a failure to pop
				// an error notification for.
				if (!(e instanceof CoreDllError && e.code === 'CANCELED')) {
					reportException(e);
				}
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
		},
		// User-data export: aggregate the selected data (the core), then save the
		// returned archive to a file the user chooses. The host owns the file I/O.
		exportUserData: async message => {
			const data: ExportUserDataData = message.data;

			let reply: ExportUserDataReplyData;
			try {
				const result = await this._utils.core.exportUserData({
					selection: data.selection,
					options: data.options,
				});

				const uri = await vscode.window.showSaveDialog({
					saveLabel: 'Export',
					// Seed a self-describing, chronologically-sorting default name under the
					// home directory (showSaveDialog couples the name with a folder).
					defaultUri: vscode.Uri.file(path.join(os.homedir(), defaultBackupFileName())),
					filters: { 'Windhawk user data': ['json'], 'All files': ['*'] },
				});
				if (!uri) {
					// A dismissed Save dialog is a benign no-op.
					reply = { succeeded: false, canceled: true };
				} else {
					fs.writeFileSync(uri.fsPath, result.archive);
					reply = { succeeded: true, summary: result.summary };
				}
			} catch (e) {
				reportException(e);
				reply = { succeeded: false };
			}

			webviewIPC.exportUserDataReply(this._webview, message.messageId, reply);
		},
		// User-data inspect: validate an archive and project its manifest (the parts
		// available to import). The webview either hands over the archive text it
		// holds, or leaves the pick to us: an Open dialog plus a read. The bytes are
		// echoed back either way, so a follow-up import needs no second read.
		inspectUserData: async message => {
			const data: InspectUserDataData = message.data;

			let reply: InspectUserDataReplyData;
			try {
				// null marks a dismissed Open dialog, which only the host-picked path
				// can produce.
				let archive: string | null | undefined = data.archive;
				if (archive === undefined) {
					const uris = await vscode.window.showOpenDialog({
						canSelectMany: false,
						openLabel: 'Import',
						filters: { 'Windhawk user data': ['json'], 'All files': ['*'] },
					});
					archive = !uris || uris.length === 0
						? null
						: readArchiveFile(uris[0].fsPath);
				}
				if (archive === null) {
					// A dismissed Open dialog is a benign no-op.
					reply = { succeeded: false, canceled: true };
				} else {
					const manifest = await this._utils.core.inspectUserData(archive);
					reply = { succeeded: true, manifest, archive };
				}
			} catch (e) {
				reportException(e);
				reply = { succeeded: false };
			}

			webviewIPC.inspectUserDataReply(this._webview, message.messageId, reply);
		},
		// User-data import: install the archive's mods and restore config/settings
		// and app settings, per the selection. Per-mod progress streams as events;
		// the terminal reply carries the outcome summary. Cancelable.
		importUserData: async message => {
			const data: ImportUserDataData = message.data;

			let reply: ImportUserDataReplyData;
			const importOp = this._utils.core.importUserData(
				{
					archive: data.archive,
					selection: data.selection,
					options: data.options,
				},
				{
					onProgress: progress => {
						webviewIPC.importUserDataProgress(this._webview, progress);

						// The archive's app settings are on disk once the step reports
						// applied, so announce them from here rather than after the
						// import returns: they are applied BEFORE the mod loop, which
						// can run for minutes, and they stay applied even if a later
						// mod cancels the import. Fire-and-forget with its own catch -
						// this is a void event callback, and a failed announcement must
						// not fail the import that already applied them.
						if (progress.item === 'appSettings' && progress.status === 'applied') {
							this._announceAppSettings().catch(e => reportException(e));
						}
					},
				},
			);
			this._currentImportOp = importOp;

			try {
				const result = await importOp.result;

				// Restart or poke the engine for the imported app settings, like saving
				// advanced app settings does (see the updateAppSettings handler). A tray
				// failure surfaces as succeeded:false, the same as there.
				const intents = result.summary.appSettings;
				if (intents?.requiresRestart) {
					await this._utils.core.notifyTray('restartBg');
				} else if (intents?.requiresNotify) {
					await this._utils.core.notifyTray('appSettingsChanged');
				}

				reply = { succeeded: true, summary: result.summary };
			} catch (e) {
				// A user-initiated cancel rejects with CANCELED: an acknowledged
				// outcome, not a failure to pop an error notification for (the
				// import dialog shows what completed before the cancel).
				if (!(e instanceof CoreDllError && e.code === 'CANCELED')) {
					reportException(e);
				}
				reply = { succeeded: false };
				// A missing-dev-tools fail-fast: surface its code so the webview raises
				// the install-dev-tools prompt (the native host attaches this already).
				if (e instanceof CoreDllError && e.code === 'DEV_TOOLS_MISSING') {
					reply.error = { code: e.code, message: e.message };
				}
			}

			webviewIPC.importUserDataReply(this._webview, message.messageId, reply);
		},
		cancelImportUserData: message => {
			let succeeded = false;
			try {
				// cancel() of a finished (or never-started) import is a harmless
				// no-op returning false, like cancelUpdate.
				if (this._currentImportOp?.cancel()) {
					succeeded = true;
				}
			} catch (e) {
				reportException(e);
			}

			webviewIPC.cancelImportUserDataReply(this._webview, message.messageId, {
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
		// Record the catalog's latest versions in the user profile. The tray
		// picks the new versions up through its own watcher on the profile
		// file, so nothing is posted to it here.
		await this._utils.core.syncCatalogToProfile(catalog);
		return catalog.mods;
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
				contractVersion: WEBVIEW_IPC_CONTRACT_VERSION,
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
				const result = await compileOp.result;

				if (modId !== oldModId) {
					this._utils.editorWorkspace.setEditorModeModId(modId);

					this._editedModId = modId;
					webviewIPC.setEditedModId(this._view?.webview, {
						modId
					});
				}

				// A successful compile may still have emitted warnings; keep them in
				// the compiler-output channel regardless of what panel is revealed.
				const hasWarnings = appendCompilerWarnings(result.warnings);

				// An omitted loggingEnabled preserves the mod's existing state, so
				// query the installed config to learn whether logging ended up on.
				let loggingEnabled = data.loggingEnabled;
				if (loggingEnabled === undefined) {
					const modConfig = await this._utils.core.getModConfig(localModId);
					loggingEnabled = modConfig?.loggingEnabled ?? false;
				}

				if (loggingEnabled) {
					windhawkLogOutput?.createOrShow(true);
				} else if (hasWarnings) {
					// Reveal the warnings without stealing focus from the editor.
					windhawkCompilerOutput?.show(true);
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

// The default file name a user-data export suggests: a local timestamp plus a
// windhawk-backup base (e.g. 2020-12-25-14h30m05-windhawk-backup.json), so exports
// are self-describing and sort chronologically. The archive's own exportedAt is
// core-stamped in UTC and independent of this name.
function defaultBackupFileName(): string {
	const now = new Date();
	const p = (n: number) => String(n).padStart(2, '0');
	return (
		`${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}-` +
		`${p(now.getHours())}h${p(now.getMinutes())}m${p(now.getSeconds())}-windhawk-backup.json`
	);
}

// Read a picked archive file, refusing one past the core's cap by its SIZE first:
// the read pulls the whole document into memory, so a file that cannot be a valid
// archive must be rejected before it is read rather than after. The message is
// worded like the core's own rejection, and the caller's catch surfaces it.
function readArchiveFile(filePath: string): string {
	const { size } = fs.statSync(filePath);
	if (size > MAX_ARCHIVE_BYTES) {
		throw new Error(
			`archive is too large (${size} bytes; the maximum is ${MAX_ARCHIVE_BYTES})`
		);
	}
	return fs.readFileSync(filePath, 'utf8');
}

function reportException(e: any) {
	console.error(e);
	vscode.window.showErrorMessage(e.message);
}

// Surface each failed parseModSource section as its own error notification,
// leaving the other sections' parsing unaffected.
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

// Surface the clang warnings a successful local compile still produced. Append
// them to the compiler-output channel and return whether anything was written,
// so the caller can decide whether to reveal the channel (without stealing
// focus). A clean compile or a precompiled download carries no warnings, so this
// is a no-op there.
function appendCompilerWarnings(warnings: string | undefined): boolean {
	if (!warnings) {
		return false;
	}
	windhawkCompilerOutput?.append(warnings + '\n');
	return true;
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

	// The base href, the CSP and the body markers are injected by patching the
	// bundled template. A template change which drops either anchor tag has to
	// fail here, not silently produce a webview without a CSP.
	const headTag = '<head>';
	if (!html.includes(headTag)) {
		throw new Error('The webview template has no <head> tag to inject the Content-Security-Policy into');
	}

	html = html.replace(headTag, `<head>
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

	const bodyTagRegex = /<body([^>]*)>/;
	if (!bodyTagRegex.test(html)) {
		throw new Error('The webview template has no <body> tag to inject the webview parameters into');
	}

	html = html.replace(bodyTagRegex, `<body data-content="${bodyDataContent}"${dataParams}${dataVscodeContext}$1>`);

	return html;
}
