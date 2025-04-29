import * as child_process from 'child_process';
import * as vscode from 'vscode';

export class WindhawkLogOutput {
	private _logOutputProcessPath: string;
	private _logOutputChannel?: vscode.OutputChannel;
	private _logOutputProcess?: child_process.ChildProcessWithoutNullStreams;
	private _incompleteStdoutBuffer: Buffer = Buffer.alloc(0);
	private _incompleteStderrBuffer: Buffer = Buffer.alloc(0);

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
				const dataWithIncompleteBuffer = Buffer.concat([this._incompleteStdoutBuffer, data]);
				const index = incompleteUTF8Index(dataWithIncompleteBuffer);
				const dataToOutput = dataWithIncompleteBuffer.subarray(0, index);
				this._incompleteStdoutBuffer = dataWithIncompleteBuffer.subarray(index);
				this._logOutputChannel?.append(dataToOutput.toString());
			});

			ps.stderr.on('data', data => {
				const dataWithIncompleteBuffer = Buffer.concat([this._incompleteStderrBuffer, data]);
				const index = incompleteUTF8Index(dataWithIncompleteBuffer);
				const dataToOutput = dataWithIncompleteBuffer.subarray(0, index);
				this._incompleteStderrBuffer = dataWithIncompleteBuffer.subarray(index);
				this._logOutputChannel?.append(dataToOutput.toString());
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

// Inspired by https://stackoverflow.com/a/27587200
function incompleteUTF8Index(buf: Buffer) {
    for (let i = Math.max(buf.length - 3, 0); i < buf.length; i++) {
        const ch = buf[i];
        if ((ch & 0xc0) === 0x80) {
            // 10xxxxxx
            continue;
        }

        const leadIndex = i;
        if ((ch & 0x80) === 0) {
            // 0xxxxxxx
        } else if ((ch & 0xe0) === 0xc0) {
            // 110xxxxx
            i++;
        } else if ((ch & 0xf0) === 0xe0) {
            // 1110xxxx
            i += 2;
        } else if ((ch & 0xf8) === 0xf0) {
            // 11110xxx
            i += 3;
        } else {
            // Unrecognized.
            break;
        }

        if (i >= buf.length) {
            return leadIndex;
        }
    }

    return buf.length;
}
