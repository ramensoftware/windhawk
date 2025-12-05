import { spawn } from 'child_process';
import * as crypto from 'crypto';
import * as fs from 'fs';
import fetch from 'node-fetch';
import * as os from 'os';
import * as path from 'path';

export interface UpdateProgress {
	progress: number; // 0-100
}

export interface UpdateCallbacks {
	onProgress: (data: UpdateProgress) => void;
	onInstalling: () => void;
}

export interface UpdateResult {
	succeeded: boolean;
	error?: string;
}

export class UpdateUtils {
	private _isUpdating = false;
	private _downloadAbortController: AbortController | null = null;
	private _tempFolderPath: string | null = null;
	private _tempInstallerPath: string | null = null;

	constructor(
		private readonly _isPortable: boolean,
		private readonly _appRootPath: string
	) { }

	public isUpdating(): boolean {
		return this._isUpdating;
	}

	public async startUpdate(callbacks: UpdateCallbacks): Promise<UpdateResult> {
		if (this._isUpdating) {
			return { succeeded: false, error: 'Update is already in progress' };
		}

		this._isUpdating = true;

		try {
			// Download the latest installer
			await this._downloadInstaller(callbacks);

			// Start installation
			callbacks.onInstalling();
			await this._installUpdate();

			// At this point, the installer was started successfully
			return { succeeded: true };
		} catch (error: any) {
			return {
				succeeded: false,
				error: error.message || 'Unknown error occurred'
			};
		} finally {
			this._cleanup();
		}
	}

	public cancelUpdate(): boolean {
		if (!this._isUpdating || !this._downloadAbortController) {
			return false;
		}

		// Just abort the download. startUpdate() will handle cleanup.
		this._downloadAbortController.abort();
		return true;
	}

	private async _downloadInstaller(callbacks: UpdateCallbacks): Promise<void> {
		const installerUrl = 'https://github.com/ramensoftware/windhawk/releases/latest/download/windhawk_setup.exe';

		this._downloadAbortController = new AbortController();

		try {
			// Create a random subfolder inside os.tmpdir to avoid DLL hijacking
			const randomFolderName = `windhawk_update_${crypto.randomBytes(8).toString('hex')}`;
			this._tempFolderPath = path.join(os.tmpdir(), randomFolderName);
			fs.mkdirSync(this._tempFolderPath, { recursive: true });

			this._tempInstallerPath = path.join(this._tempFolderPath, 'windhawk_setup.exe');

			const response = await fetch(installerUrl, {
				signal: this._downloadAbortController.signal
			});

			if (!response.ok) {
				this._downloadAbortController = null;
				throw new Error(`Failed to download update: ${response.statusText || response.status}`);
			}

			const totalSize = parseInt(response.headers.get('content-length') || '0', 10);
			let downloadedSize = 0;
			let lastReportedProgress = -1;

			const fileStream = fs.createWriteStream(this._tempInstallerPath);

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
			let args: string[];

			if (this._isPortable) {
				args = ['/PORTABLE', '/AUTO_UPDATE', '/LANG=1033', `/D=${this._appRootPath}`];
			} else {
				args = ['/AUTO_UPDATE'];
			}

			// Run the installer with appropriate flags
			// The installer should handle restarting Windhawk
			const installerProcess = spawn(tempInstallerPath, args, {
				detached: true,
				stdio: 'ignore'
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

	private _cleanup(): void {
		this._isUpdating = false;
		this._downloadAbortController = null;

		if (this._tempInstallerPath) {
			try {
				fs.unlinkSync(this._tempInstallerPath);
			} catch (error) {
				// Ignore cleanup errors
			}
		}

		if (this._tempFolderPath) {
			try {
				fs.rmdirSync(this._tempFolderPath);
			} catch (error) {
				// Ignore cleanup errors
			}
		}

		this._tempInstallerPath = null;
		this._tempFolderPath = null;
	}
}
