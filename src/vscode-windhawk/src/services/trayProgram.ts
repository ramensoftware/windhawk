import * as child_process from 'child_process';
import * as path from 'path';
import { Logger } from './logger';

export default class TrayProgram {
	private trayProgramPath: string;
	private logger: Logger;

	public constructor(appRootPath: string, logger: Logger) {
		this.trayProgramPath = path.join(appRootPath, 'windhawk.exe');
		this.logger = logger;
	}

	private runTrayProgramWithArgs(args: string[]) {
		try {
			const ps = child_process.spawn(this.trayProgramPath, args);

			let gotError = false;

			ps.on('error', err => {
				//console.log('Oh no, the error: ' + err);
				gotError = true;
				this.logger.error(err.message);
			});

			ps.on('close', code => {
				//console.log(`ps process exited with code ${code}`);
				if (!gotError && code !== 0) {
					this.logger.warn('Communication with the Windhawk tray icon process failed, make sure it\'s running');
				}
			});
		} catch (e) {
			this.logger.error(e.message);
		}
	}

	public postAppRestartBg() {
		this.runTrayProgramWithArgs([
			'-restart-bg'
		]);
	}

	public postNewUpdatesFound() {
		this.runTrayProgramWithArgs([
			'-new-updates-found'
		]);
	}

	public postAppSettingsChanged() {
		this.runTrayProgramWithArgs([
			'-app-settings-changed'
		]);
	}
}
