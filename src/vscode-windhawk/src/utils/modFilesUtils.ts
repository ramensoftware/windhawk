import * as fs from 'fs';
import * as https from 'https';
import fetch from 'node-fetch';
import * as path from 'path';
import * as semver from 'semver';

type ArchitectureSubfolder = '32' | '64' | 'arm64';

export default class ModFilesUtils {
	private engineModsPath: string;
	private arm64Enabled: boolean;
	private currentWindhawkVersion: semver.SemVer | null;

	public constructor(appDataPath: string, arm64Enabled: boolean, currentWindhawkVersion: semver.SemVer | null) {
		this.engineModsPath = path.join(appDataPath, 'Engine', 'Mods');
		this.arm64Enabled = arm64Enabled;
		this.currentWindhawkVersion = currentWindhawkVersion;
	}

	/**
	 * Converts metadata architecture strings to DLL subfolders.
	 * Handles special cases like x86-64 expanding to both 64 and arm64 when ARM64 is enabled.
	 */
	private subfoldersFromArchitectures(architectures: string[]): Set<ArchitectureSubfolder> {
		const subfolders = new Set<ArchitectureSubfolder>();

		// Default to x86 and x86-64 if no architectures specified
		const archsToProcess = (architectures.length > 0) ? architectures : ['x86', 'x86-64'];

		for (const arch of archsToProcess) {
			switch (arch) {
				case 'x86':
					subfolders.add('32');
					break;
				case 'x86-64':
					// x86-64 means "64-bit" for compatibility, which could be
					// either x64 or ARM64
					if (this.arm64Enabled) {
						subfolders.add('64');
						subfolders.add('arm64');
					} else {
						subfolders.add('64');
					}
					break;
				case 'amd64':
					// Explicitly x64, not ARM64
					subfolders.add('64');
					break;
				case 'arm64':
					if (this.arm64Enabled) {
						subfolders.add('arm64');
					}
					break;
				default:
					throw new Error(`Unsupported architecture: ${arch}`);
			}
		}

		return subfolders;
	}

	private deleteOldModFilesInFolder(modId: string, subfolder: ArchitectureSubfolder, currentDllName?: string) {
		const compiledModsPath = path.join(this.engineModsPath, subfolder);

		let compiledModsDir: fs.Dir;
		try {
			compiledModsDir = fs.opendirSync(compiledModsPath);
		} catch (e: any) {
			// Ignore if directory doesn't exist.
			if (e.code !== 'ENOENT') {
				throw e;
			}
			return;
		}

		try {
			let compiledModsDirEntry: fs.Dirent | null;
			while ((compiledModsDirEntry = compiledModsDir.readSync()) !== null) {
				if (!compiledModsDirEntry.isFile()) {
					continue;
				}

				const filename = compiledModsDirEntry.name;
				if (currentDllName && filename === currentDllName) {
					continue;
				}

				if (!filename.startsWith(modId + '_') || !filename.endsWith('.dll')) {
					continue;
				}

				const filenamePart = filename.slice((modId + '_').length, -'.dll'.length);
				if (!filenamePart.match(/(^|_)[0-9]+$/)) {
					continue;
				}

				const compiledModPath = path.join(compiledModsPath, filename);

				try {
					fs.unlinkSync(compiledModPath);
				} catch (e) {
					// Ignore errors (file may be in use).
				}
			}
		} finally {
			compiledModsDir.closeSync();
		}
	}

	public deleteOldModFiles(modId: string, architectures: string[], currentDllName?: string) {
		const subfolders = this.subfoldersFromArchitectures(architectures);

		for (const subfolder of subfolders) {
			this.deleteOldModFilesInFolder(modId, subfolder, currentDllName);
		}
	}

	public async downloadPrecompiledMod(
		modId: string,
		version: string,
		architectures: string[],
		modsUrl: string
	): Promise<{ targetDllName: string }> {
		// Generate a unique DLL name
		const targetDllName = modId + '_' + version + '_' + randomIntFromInterval(100000, 999999) + '.dll';

		const subfolders = this.subfoldersFromArchitectures(architectures);
		if (subfolders.size === 0) {
			throw new Error('The current architecture is not supported');
		}

		// Collect URLs and target paths
		const downloads: Array<{ subfolder: ArchitectureSubfolder; url: string; targetPath: string }> = [];
		for (const subfolder of subfolders) {
			const url = `${modsUrl}${modId}/${version}_${subfolder}.dll`;
			const targetPath = path.join(this.engineModsPath, subfolder, targetDllName);
			downloads.push({ subfolder, url, targetPath });
		}

		// Collect all URLs to fetch (DLLs + versions.json)
		const versionsJsonUrl = `${modsUrl}${modId}/versions.json`;
		const urlsToFetch = [...downloads.map(d => d.url), versionsJsonUrl];

		// Create HTTP agent with keepAlive to reuse connections
		const agent = new https.Agent({ keepAlive: true });

		// Fetch all URLs in parallel with shared agent
		const responses = await Promise.all(urlsToFetch.map(url => fetch(url, { agent })));

		// Clean up the agent after all requests complete
		agent.destroy();

		// Check all responses succeeded
		for (let i = 0; i < downloads.length; i++) {
			const response = responses[i];
			if (!response.ok) {
				throw new Error(`Failed to download ${downloads[i].subfolder} DLL: ${response.statusText || response.status}`);
			}
		}

		// Check versions.json response
		const versionsJsonResponse = responses[responses.length - 1];
		if (!versionsJsonResponse.ok) {
			throw new Error(`Failed to download versions.json: ${versionsJsonResponse.statusText || versionsJsonResponse.status}`);
		}

		// Check minimum Windhawk version requirement
		const versionsJsonText = await versionsJsonResponse.json();
		const versionInfo = versionsJsonText.find((v: any) => v.version === version);
		const minWindhawkVersion = versionInfo?.minWindhawkVersion;

		if (minWindhawkVersion) {
			let currentVersion = this.currentWindhawkVersion;
			const requiredVersion = semver.coerce(minWindhawkVersion);
			if (currentVersion && requiredVersion && semver.lt(currentVersion, requiredVersion)) {
				throw new Error(
					`Mod version ${version} requires Windhawk ${minWindhawkVersion} or later, ` +
					`but current version is ${currentVersion.version}`
				);
			}
		}

		try {
			// Write all DLL buffers
			const dllResponses = responses.slice(0, downloads.length);
			const buffers = await Promise.all(dllResponses.map(r => r.buffer()));
			for (let i = 0; i < buffers.length; i++) {
				fs.mkdirSync(path.dirname(downloads[i].targetPath), { recursive: true });
				fs.writeFileSync(downloads[i].targetPath, buffers[i]);
			}
		} catch (e: any) {
			// Clean up any partially downloaded files
			for (const cleanupSubfolder of subfolders) {
				const cleanupPath = path.join(this.engineModsPath, cleanupSubfolder, targetDllName);
				try {
					if (fs.existsSync(cleanupPath)) {
						fs.unlinkSync(cleanupPath);
					}
				} catch (cleanupError) {
					// Ignore cleanup errors
				}
			}
			throw e;
		}

		return { targetDllName };
	}

	public deleteModFiles(modId: string) {
		// Delete all files for all architectures
		const allSubfolders: ArchitectureSubfolder[] = ['32', '64'];
		if (this.arm64Enabled) {
			allSubfolders.push('arm64');
		}

		for (const subfolder of allSubfolders) {
			this.deleteOldModFilesInFolder(modId, subfolder);
		}
	}
}

// https://stackoverflow.com/a/7228322
// min and max included
function randomIntFromInterval(min: number, max: number) {
	return Math.floor(Math.random() * (max - min + 1) + min);
}
