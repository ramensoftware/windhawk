import * as fs from 'fs';
import * as path from 'path';
import * as child_process from 'child_process';
import * as vscode from 'vscode';
import config from '../config';

// The charset the core enforces on a mod's @id (domain validate_metadata), kept
// here because ids reach paths by plain interpolation and sanitize nothing.
function isValidModId(modId: string) {
	return /^[0-9a-z-]+$/.test(modId);
}

export default class EditorWorkspaceUtils {
	private workspacePath: string;

	public constructor() {
		const firstWorkspaceFolder = vscode.workspace.workspaceFolders?.[0];
		if (!firstWorkspaceFolder) {
			vscode.commands.executeCommand('workbench.action.files.openFolder');
			throw new Error('No workspace folder');
		}

		this.workspacePath = firstWorkspaceFolder.uri.fsPath;
	}

	public getFilePath(fileName: string) {
		return path.join(this.workspacePath, fileName);
	}

	public getWorkspaceFolder() {
		return this.workspacePath;
	}

	public getModSourcePath() {
		return this.getFilePath('mod.wh.cpp');
	}

	public writePchHeader(content: string) {
		fs.writeFileSync(this.getFilePath('windhawk_pch.h'), content);
	}

	// The precompiled-header source and the per-target caches the core builds
	// from it. Returns whether there was anything to delete.
	public deletePchFiles() {
		const pchFiles = fs.readdirSync(this.workspacePath).filter(
			name => name === 'windhawk_pch.h' || /^windhawk_t_.+\.pch$/.test(name)
		);

		for (const name of pchFiles) {
			fs.unlinkSync(this.getFilePath(name));
		}

		return pchFiles.length > 0;
	}

	public getDraftsPath() {
		return path.join(this.workspacePath, 'Drafts');
	}

	private initializeEditorSettings(compileFlags: string[]) {
		// Flags for clangd, provided by the core so they stay in sync with the
		// real compiler flags (core.getCompileFlags()).
		fs.writeFileSync(this.getFilePath('compile_flags.txt'), compileFlags.join('\n') + '\n');

		const clangFormatConfig = [
			'# To override, create a .clang-format.windhawk file with the desired settings.',
			'BasedOnStyle: Chromium',
			'IndentWidth: 4',
			'CommentPragmas: ^[ \\t]+@[a-zA-Z]+',
		];

		if (fs.existsSync(this.getFilePath('.clang-format.windhawk'))) {
			fs.copyFileSync(this.getFilePath('.clang-format.windhawk'), this.getFilePath('.clang-format'));
		} else {
			fs.writeFileSync(this.getFilePath('.clang-format'), clangFormatConfig.join('\n') + '\n');
		}

		if (!fs.existsSync(this.getFilePath('.git'))) {
			child_process.spawnSync('git', ['init'], { cwd: this.workspacePath, stdio: 'ignore' });
		}

		if (fs.existsSync(this.getFilePath('.git'))) {
			child_process.spawnSync('git', ['add', 'mod.wh.cpp'], { cwd: this.workspacePath, stdio: 'ignore' });
		}
	}

	public initializeFromModSource(modSource: string, compileFlags: string[], modSourceFromDrafts?: string | null) {
		fs.writeFileSync(this.getFilePath('mod.wh.cpp'), modSource);

		// Remove windhawk_api.h from older versions, it now resides in the
		// compiler include folder.
		try {
			fs.unlinkSync(this.getFilePath('windhawk_api.h'));
		} catch (e) {
			// Ignore if file doesn't exist.
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}

		this.initializeEditorSettings(compileFlags);

		if (modSourceFromDrafts) {
			// Write the new content after initializing, so that git won't stage the draft changes.
			fs.writeFileSync(this.getFilePath('mod.wh.cpp'), modSourceFromDrafts);
		}
	}

	// Reject an id that would escape the drafts folder before it reaches any of
	// the filesystem calls below.
	private getDraftPath(modId: string) {
		if (!isValidModId(modId)) {
			throw new Error('Mod id must only contain the following characters: 0-9, a-z, and a hyphen (-)');
		}

		return path.join(this.getDraftsPath(), modId + '.wh.cpp');
	}

	public saveModToDrafts(modId: string) {
		const modSourcePath = this.getDraftPath(modId);
		fs.mkdirSync(this.getDraftsPath(), { recursive: true });
		fs.copyFileSync(this.getFilePath('mod.wh.cpp'), modSourcePath);
	}

	public loadModFromDrafts(modId: string) {
		const modSourcePath = this.getDraftPath(modId);
		if (fs.existsSync(modSourcePath)) {
			return fs.readFileSync(modSourcePath, 'utf8');
		}

		return null;
	}

	public deleteModFromDrafts(modId: string) {
		const modSourcePath = this.getDraftPath(modId);
		try {
			fs.unlinkSync(modSourcePath);
		} catch (e) {
			// Ignore if file doesn't exist.
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}
	}

	public async openModSource() {
		await vscode.commands.executeCommand('vscode.open', vscode.Uri.file(this.getModSourcePath()), {
			preview: false
		});
	}

	private async toggleMinimalLayout(minimal: boolean) {
		const vscodeConfig = vscode.workspace.getConfiguration();
		const thenableArray: Thenable<void>[] = [];

		if (minimal) {
			thenableArray.push(vscode.commands.executeCommand('workbench.action.closeSidebar'));
			thenableArray.push(vscode.commands.executeCommand('workbench.action.closePanel'));
			thenableArray.push(vscodeConfig.update('workbench.activityBar.visible', false));
		} else if (process.env['WINDHAWK_UI_EDITOR_ACTIVITY_BAR_VISIBLE'] === '1') {
			thenableArray.push(vscodeConfig.update('workbench.activityBar.visible', true));
		}

		thenableArray.push(vscodeConfig.update('workbench.editor.showTabs', !minimal));
		thenableArray.push(vscodeConfig.update('workbench.statusBar.visible', !minimal));

		return Promise.all(thenableArray);
	}

	public async enterEditorMode(modId: string, modWasModified = false) {
		const vscodeConfig = vscode.workspace.getConfiguration();
		await Promise.all([
			vscodeConfig.update('windhawk.editedModId', modId),
			vscodeConfig.update('windhawk.editedModWasModified', modWasModified),
			vscodeConfig.update('git.enabled', true)
		]);

		const modSourceUri = vscode.Uri.file(this.getModSourcePath());
		const modFileAlreadyOpen = vscode.window.visibleTextEditors.some(
			editor => editor.document.uri.toString(true) === modSourceUri.toString(true)
		);
		if (!modFileAlreadyOpen) {
			await this.openModSource();
			await vscode.commands.executeCommand('workbench.action.closeEditorsInOtherGroups');
			await vscode.commands.executeCommand('workbench.action.closeOtherEditors');
		}

		await vscode.commands.executeCommand('windhawk.sidebar.focus', {
			preserveFocus: true
		});

		if (!config.debug.disableMinimalMode) {
			await this.toggleMinimalLayout(false);
		}
	}

	public async exitEditorMode() {
		const vscodeConfig = vscode.workspace.getConfiguration();
		await Promise.all([
			vscodeConfig.update('windhawk.editedModId', undefined),
			vscodeConfig.update('windhawk.editedModWasModified', undefined),
			vscodeConfig.update('git.enabled', undefined),
		]);

		await vscode.commands.executeCommand('windhawk.start');
		await vscode.commands.executeCommand('workbench.action.closeEditorsInOtherGroups');
		await vscode.commands.executeCommand('workbench.action.closeOtherEditors');

		if (!config.debug.disableMinimalMode) {
			await this.toggleMinimalLayout(true);
		}
	}

	public async restoreEditorMode() {
		const vscodeConfig = vscode.workspace.getConfiguration();
		// The setting is the one mod id that isn't handed over by the core, so
		// treat a malformed one as no editor mode rather than carrying it to the
		// drafts helpers, which would leave editor mode impossible to exit.
		const modIdConfig = vscodeConfig.get('windhawk.editedModId');
		const modId = typeof modIdConfig === 'string' && isValidModId(modIdConfig) ? modIdConfig : null;

		if (modId) {
			const modWasModified = !!vscodeConfig.get('windhawk.editedModWasModified');
			await this.enterEditorMode(modId, modWasModified);
			return {
				modId,
				modWasModified
			};
		} else {
			await this.exitEditorMode();
			return {
				modId: null
			};
		}
	}

	public async setEditorModeModId(modId: string) {
		const vscodeConfig = vscode.workspace.getConfiguration();
		await vscodeConfig.update('windhawk.editedModId', modId);
	}

	public async markEditorModeModAsModified(modified: boolean) {
		if (!modified && fs.existsSync(this.getFilePath('.git'))) {
			const gitAdd = child_process.spawn('git', ['add', 'mod.wh.cpp'], { cwd: this.workspacePath, stdio: 'ignore' });
			// The .git check only says git worked here once, not that it can be
			// launched. Staging is best effort, and an 'error' event with no
			// listener is rethrown into the extension host.
			gitAdd.on('error', () => {});
		}

		const vscodeConfig = vscode.workspace.getConfiguration();
		await vscodeConfig.update('windhawk.editedModWasModified', modified);
	}
}
