import * as child_process from 'child_process';
import * as vscode from 'vscode';

export class WindhawkLogOutput {
	private _logOutputProcessPath: string;
	private _logOutputChannel?: vscode.OutputChannel;
	private _logOutputProcess?: child_process.ChildProcessWithoutNullStreams;

	constructor(logOutputProcessPath: string) {
		this._logOutputProcessPath = logOutputProcessPath;
	}

	public createOrShow(preserveFocus?: boolean) {
		if (!this._logOutputChannel) {
			this._logOutputChannel = vscode.window.createOutputChannel('Windhawk Log');
		}
		this._logOutputChannel.show(preserveFocus);

		if (!this._logOutputProcess) {
			const args = [
				'--pattern',
				'[WH] *',
				'--no-buffering',
			];
			const ps = child_process.spawn(this._logOutputProcessPath, args);

			this._logOutputProcess = ps;

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
				this._logOutputProcess = undefined;
				gotError = true;
				vscode.window.showErrorMessage(err.message);
			});

			ps.on('close', code => {
				//console.log(`ps process exited with code ${code}`);
				if (!gotError) {
					this._logOutputProcess = undefined;
				}
			});
		}
	}

	public dispose() {
		if (this._logOutputProcess) {
			this._logOutputProcess.kill();
			this._logOutputProcess = undefined;
		}

		if (this._logOutputChannel) {
			this._logOutputChannel.dispose();
			this._logOutputChannel = undefined;
		}
	}
}
