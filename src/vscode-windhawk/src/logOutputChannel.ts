import * as child_process from 'child_process';
import * as vscode from 'vscode';

export class WindhawkLogOutput {
	private _debugPlusPlusPath: string;
	private _logOutputChannel?: vscode.OutputChannel;
	private _debugPlusPlus?: child_process.ChildProcessWithoutNullStreams;

	constructor(debugPlusPlusPath: string) {
		this._debugPlusPlusPath = debugPlusPlusPath;
	}

	public createOrShow(preserveFocus?: boolean) {
		if (!this._logOutputChannel) {
			this._logOutputChannel = vscode.window.createOutputChannel('Windhawk Log');
		}
		this._logOutputChannel.show(preserveFocus);

		if (!this._debugPlusPlus) {
			const args = [
				'-i', // include filter
				'[WH]',
				'-a', // auto-newline
				'-c', // enable console output
				'-s', // prefix messages with system time
				'-p', // add PID (process ID)
				'-n', // add process name
				'-f', // aggressively flush buffers
			];
			const ps = child_process.spawn(this._debugPlusPlusPath, args);

			this._debugPlusPlus = ps;

			ps.stdout.on('data', data => {
				//console.log(`ps stdout: ${data}`);
				this._logOutputChannel?.append(data.toString());
			});

			ps.stderr.on('data', data => {
				//console.log(`ps stderr: ${data}`);
				this._logOutputChannel?.append(data.toString());
			});

			let gotError = false;

			ps.on('error', err => {
				//console.log('Oh no, the error: ' + err);
				this._debugPlusPlus = undefined;
				gotError = true;
				vscode.window.showErrorMessage(err.message);
			});

			ps.on('close', code => {
				//console.log(`ps process exited with code ${code}`);
				if (!gotError) {
					this._debugPlusPlus = undefined;
				}
			});
		}
	}

	public dispose() {
		if (this._debugPlusPlus) {
			this._debugPlusPlus.kill();
			this._debugPlusPlus = undefined;
		}

		if (this._logOutputChannel) {
			this._logOutputChannel.dispose();
			this._logOutputChannel = undefined;
		}
	}
}
