import * as fs from 'fs';
import * as path from 'path';
import * as child_process from 'child_process';
import * as vscode from 'vscode';
import config from '../config';

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

	public getDraftsPath() {
		return path.join(this.workspacePath, 'Drafts');
	}

	private initializeEditorSettings() {
		// Flags for clangd.
		const compileFlags = [
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

	public initializeFromModSource(modSource: string, modSourceFromDrafts?: string | null) {
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

		this.initializeEditorSettings();

		if (modSourceFromDrafts) {
			// Write the new content after initializing, so that git won't stage the draft changes.
			fs.writeFileSync(this.getFilePath('mod.wh.cpp'), modSourceFromDrafts);
		}
	}

	public saveModToDrafts(modId: string) {
		const draftsDir = this.getDraftsPath();
		fs.mkdirSync(draftsDir, { recursive: true });
		fs.copyFileSync(this.getFilePath('mod.wh.cpp'), path.join(draftsDir, modId + '.wh.cpp'));
	}

	public loadModFromDrafts(modId: string) {
		const draftsPath = this.getDraftsPath();
		const modSourcePath = path.join(draftsPath, modId + '.wh.cpp');
		if (fs.existsSync(modSourcePath)) {
			return fs.readFileSync(modSourcePath, 'utf8');
		}

		return null;
	}

	public deleteModFromDrafts(modId: string) {
		const draftsPath = this.getDraftsPath();
		const modSourcePath = path.join(draftsPath, modId + '.wh.cpp');
		try {
			fs.unlinkSync(modSourcePath);
		} catch (e) {
			// Ignore if file doesn't exist.
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}
	}

	private async toggleMinimalLayout(minimal: boolean) {
		const vscodeConfig = vscode.workspace.getConfiguration();
		const thenableArray: Thenable<void>[] = [];

		if (minimal) {
			thenableArray.push(vscode.commands.executeCommand('workbench.action.closeSidebar'));
			thenableArray.push(vscode.commands.executeCommand('workbench.action.closePanel'));
			thenableArray.push(vscodeConfig.update('workbench.activityBar.visible', false));
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

		await vscode.commands.executeCommand('vscode.open', vscode.Uri.file(this.getFilePath('mod.wh.cpp')), {
			preview: false
		});
		await vscode.commands.executeCommand('workbench.action.closeEditorsInOtherGroups');
		await vscode.commands.executeCommand('workbench.action.closeOtherEditors');
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
		const modIdConfig = vscodeConfig.get('windhawk.editedModId');
		const modId = typeof modIdConfig === 'string' ? modIdConfig : null;

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
			child_process.spawn('git', ['add', 'mod.wh.cpp'], { cwd: this.workspacePath, stdio: 'ignore' });
		}

		const vscodeConfig = vscode.workspace.getConfiguration();
		await vscodeConfig.update('windhawk.editedModWasModified', modified);
	}
}
