import * as child_process from 'child_process';
import { StringDecoder } from 'string_decoder';
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

			// StringDecoder buffers any trailing partial UTF-8 sequence between
			// chunks so we never emit broken characters. One decoder per stream
			// per spawn -- state dies with the process.
			const stdoutDecoder = new StringDecoder('utf8');
			const stderrDecoder = new StringDecoder('utf8');

			ps.stdout.on('data', data => {
				this._logOutputChannel?.append(stdoutDecoder.write(data));
			});

			ps.stderr.on('data', data => {
				this._logOutputChannel?.append(stderrDecoder.write(data));
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
