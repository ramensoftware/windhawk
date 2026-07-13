import * as child_process from 'child_process';
import * as crypto from 'crypto';
import * as fs from 'fs';
import * as https from 'https';
import fetch from 'node-fetch';
import * as os from 'os';
import * as path from 'path';
import { debugIgnoreCertErrors, debugUpdateInstallerUrl } from '../storage/debugOverrides';
import { RepoUnreachableError } from './errors';

export interface UpdateProgress {
	progress: number; // 0-100
}

export interface UpdateCallbacks {
	onProgress: (data: UpdateProgress) => void;
	onInstalling: () => void;
}

export class Update {
	private _isUpdating = false;
	private _downloadAbortController: AbortController | null = null;
	private _tempFolderPath: string | null = null;
	private _tempInstallerPath: string | null = null;
	// Current download's write stream. Tracked so cancelUpdate() can close it
	// synchronously before unlinking the temp installer: Windows keeps the file
	// locked until the write handle is released, and the async close that
	// follows an aborted fetch may not have landed by the time cancelUpdate
	// needs to clean up.
	private _fileStream: fs.WriteStream | null = null;

	constructor(
		private readonly _isPortable: boolean,
		private readonly _appRootPath: string
	) { }

	public isUpdating(): boolean {
		return this._isUpdating;
	}

	// Runs the download + installer-launch flow. Returns on success; throws
	// on any failure so callers can distinguish network errors
	// (RepoUnreachableError -> CLI exit 6) from generic failures. Callers
	// that need a succeeded/error IPC shape (the extension) should wrap
	// with their own try/catch.
	public async startUpdate(callbacks: UpdateCallbacks): Promise<void> {
		if (this._isUpdating) {
			throw new Error('Update is already in progress');
		}

		this._isUpdating = true;

		try {
			await this._downloadInstaller(callbacks);
			callbacks.onInstalling();
			await this._installUpdate();
		} finally {
			this._cleanup();
		}
	}

	public cancelUpdate(): boolean {
		if (!this._isUpdating || !this._downloadAbortController) {
			return false;
		}

		this._downloadAbortController.abort();

		// Run the full cleanup synchronously so a caller returning from
		// cancelUpdate can rely on the temp files being gone. The aborted
		// fetch's own cleanup would eventually call _cleanup again via
		// startUpdate's finally, but that chain is async and callers may not
		// have time to wait for it. _cleanup is idempotent.
		this._cleanup();

		return true;
	}

	private async _downloadInstaller(callbacks: UpdateCallbacks): Promise<void> {
		const installerUrl = debugUpdateInstallerUrl()
			?? 'https://github.com/ramensoftware/windhawk/releases/latest/download/windhawk_setup.exe';

		this._downloadAbortController = new AbortController();

		try {
			// Create a random subfolder inside os.tmpdir to avoid DLL hijacking
			const randomFolderName = `windhawk_update_${crypto.randomBytes(8).toString('hex')}`;
			this._tempFolderPath = path.join(os.tmpdir(), randomFolderName);
			fs.mkdirSync(this._tempFolderPath, { recursive: true });

			this._tempInstallerPath = path.join(this._tempFolderPath, 'windhawk_setup.exe');

			const response = await fetch(installerUrl, {
				signal: this._downloadAbortController.signal,
				// Debug-only: skip TLS validation against a self-signed test
				// server (WINDHAWK_DEBUG_IGNORE_CERT_ERRORS); undefined leaves
				// node-fetch's default secure agent in place.
				agent: debugIgnoreCertErrors()
					? new https.Agent({ rejectUnauthorized: false })
					: undefined,
			});

			if (!response.ok) {
				this._downloadAbortController = null;
				throw new RepoUnreachableError(
					`Failed to download update: ${response.statusText || response.status}`,
				);
			}

			const totalSize = parseInt(response.headers.get('content-length') || '0', 10);
			let downloadedSize = 0;
			let lastReportedProgress = -1;

			const fileStream = fs.createWriteStream(this._tempInstallerPath);
			this._fileStream = fileStream;

			await new Promise<void>((resolve, reject) => {
				if (!response.body) {
					reject(new Error('Response body is null'));
					return;
				}

				let hasError = false;

				response.body.on('data', (chunk: Buffer) => {
					downloadedSize += chunk.length;
					const progress = totalSize > 0 ? Math.floor((downloadedSize / totalSize) * 100) : 0;

					// Only report progress if it changed by at least 1%
					if (progress !== lastReportedProgress) {
						lastReportedProgress = progress;
						callbacks.onProgress({ progress });
					}
				});

				response.body.pipe(fileStream);

				fileStream.on('finish', () => {
					fileStream.close();
					if (!hasError) {
						callbacks.onProgress({ progress: 100 });
						resolve();
					}
				});

				fileStream.on('error', (error) => {
					hasError = true;
					reject(error);
				});

				response.body.on('error', (error) => {
					hasError = true;
					fileStream.close();
					reject(error);
				});
			});
		} finally {
			this._downloadAbortController = null;
		}
	}

	private async _installUpdate(): Promise<void> {
		const tempInstallerPath = this._tempInstallerPath;
		if (!tempInstallerPath || !fs.existsSync(tempInstallerPath)) {
			throw new Error('Installer file not found');
		}

		return new Promise((resolve, reject) => {
			let args: string;

			if (this._isPortable) {
				args = `/PORTABLE /AUTO_UPDATE /LANG=1033 /D=${this._appRootPath}`;
			} else {
				args = '/AUTO_UPDATE';
			}

			// Run the installer with appropriate flags. The installer should
			// handle restarting Windhawk. NSIS requires /D to be the last
			// parameter and must not contain quotes, even if the path contains
			// spaces. Using windowsVerbatimArguments to pass arguments without
			// escaping.
			const installerProcess = child_process.spawn(tempInstallerPath, [args], {
				detached: true,
				stdio: 'ignore',
				windowsVerbatimArguments: true
			});

			installerProcess.on('error', (error) => {
				reject(new Error(`Failed to start installer: ${error.message}`));
			});

			// Wait for the process to actually spawn before resolving
			installerProcess.on('spawn', () => {
				// Unref so the parent process can exit
				installerProcess.unref();

				// The installer will restart Windhawk, which will close this
				// extension, so we don't wait for the process to complete
				resolve();
			});
		});
	}

	// Idempotent: nulls every field after acting on it, so repeated calls from
	// overlapping cancel + startUpdate-finally paths are no-ops. Destroy before
	// unlink because on Windows an open write handle keeps the file locked.
	private _cleanup(): void {
		this._isUpdating = false;
		this._downloadAbortController = null;

		if (this._fileStream) {
			try {
				this._fileStream.destroy();
			} catch {
				// Ignore cleanup errors.
			}
			this._fileStream = null;
		}

		if (this._tempInstallerPath) {
			try {
				fs.unlinkSync(this._tempInstallerPath);
			} catch {
				// Ignore cleanup errors.
			}
			this._tempInstallerPath = null;
		}

		if (this._tempFolderPath) {
			try {
				fs.rmdirSync(this._tempFolderPath);
			} catch {
				// Ignore cleanup errors.
			}
			this._tempFolderPath = null;
		}
	}
}
