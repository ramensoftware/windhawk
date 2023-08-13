import * as child_process from 'child_process';
import * as path from 'path';
import * as vscode from 'vscode';

export default class TrayProgramUtils {
	private trayProgramPath: string;

	public constructor(appRootPath: string) {
		this.trayProgramPath = path.join(appRootPath, 'windhawk.exe');
	}

	private getCleanProcessEnv() {
		// Return the process environment, but without any environment variables
		// that are specific to Electron or VSCode. This is because the tray
		// process might run another instance of VSCode, and these variables
		// might cause problems (e.g. ELECTRON_RUN_AS_NODE=1).
		const cleanEnv: NodeJS.ProcessEnv = {};
		for (const [key, value] of Object.entries(process.env)) {
			if (!key.startsWith('ELECTRON_') &&
				!key.startsWith('VSCODE_') &&
				!key.startsWith('WINDHAWK_')) {
				cleanEnv[key] = value;
			}
		}

		return cleanEnv;
	}

	private runTrayProgramWithArgs(args: string[]) {
		try {
			const ps = child_process.spawn(this.trayProgramPath, args, {
				env: this.getCleanProcessEnv(),
			});

			let gotError = false;

			ps.on('error', err => {
				//console.log('Oh no, the error: ' + err);
				gotError = true;
				vscode.window.showErrorMessage(err.message);
			});

			ps.on('close', code => {
				//console.log(`ps process exited with code ${code}`);
				if (!gotError && code !== 0) {
					vscode.window.showWarningMessage('Communication with the Windhawk tray icon process failed, make sure it\'s running');
				}
			});
		} catch (e) {
			vscode.window.showErrorMessage(e.message);
		}
	}

	private runTrayProgramWithArgsDetached(args: string[]) {
		try {
			const ps = child_process.spawn(this.trayProgramPath, args, {
				env: this.getCleanProcessEnv(),
				detached: true,
				stdio: 'ignore',
			});

			ps.unref();
		} catch (e) {
			vscode.window.showErrorMessage(e.message);
		}
	}

	public postAppRestart() {
		// We need to run this detached because if we don't, the process will be
		// killed when the extension is deactivated, before it has the change to
		// start a new VSCode instance.
		this.runTrayProgramWithArgsDetached([
			'-restart'
		]);
	}

	public postAppSettingsChanged() {
		this.runTrayProgramWithArgs([
			'-app-settings-changed'
		]);
	}
}
