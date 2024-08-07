import * as child_process from 'child_process';
import * as fs from 'fs';
import * as path from 'path';

type WorkspacePaths = {
	modSourcePath: string,
	stdoutOutputPath: string,
	stderrOutputPath: string
};

export class CompilerError extends Error {
	public stdoutPath: string;
	public stderrPath: string;

	constructor(stdoutPath: string, stderrPath: string) {
		super('Compilation failed, the mod might require a newer Windhawk version');
		this.stdoutPath = stdoutPath;
		this.stderrPath = stderrPath;
	}
}

export default class CompilerUtils {
	private compilerPath: string;
	private enginePath: string;
	private engineModsPath: string;

	public constructor(compilerPath: string, enginePath: string, appDataPath: string) {
		this.compilerPath = compilerPath;
		this.enginePath = enginePath;
		this.engineModsPath = path.join(appDataPath, 'Engine', 'Mods');
	}

	private doesCompiledModExist(fileName: string, bits: number) {
		const compiledModPath = path.join(this.engineModsPath, bits.toString(), fileName);
		return fs.existsSync(compiledModPath);
	}

	private async makePrecompiledHeaders(
		pchHeaderPath: string,
		targetPchPath: string,
		bits: number,
		stdoutOutputPath: string,
		stderrOutputPath: string,
		modId: string,
		modVersion: string,
		extraArgs: string[],
	): Promise<number | null> {
		const gppPath = path.join(this.compilerPath, 'bin', 'g++.exe');

		const args = [
			'-std=c++23',
			'-O2',
			'-DUNICODE',
			'-D_UNICODE',
			'-DWINVER=0x0A00',
			'-D_WIN32_WINNT=0x0A00',
			'-D_WIN32_IE=0x0A00',
			'-DNTDDI_VERSION=0x0A000008',
			'-D__USE_MINGW_ANSI_STDIO=0',
			'-DWH_MOD',
			'-DWH_MOD_ID=L"' + modId.replace(/"/g, '\\"') + '"',
			'-DWH_MOD_VERSION=L"' + modVersion.replace(/"/g, '\\"') + '"',
			'-x',
			'c++-header',
			pchHeaderPath,
			'-target',
			bits === 64 ? 'x86_64-w64-mingw32' : 'i686-w64-mingw32',
			'-o',
			targetPchPath,
			...extraArgs.filter(arg => arg.startsWith('-D'))
		];
		const ps = child_process.spawn(gppPath, args, {
			cwd: this.compilerPath
		});

		fs.writeFileSync(stdoutOutputPath, '');
		fs.writeFileSync(stderrOutputPath, '');

		ps.stdout.on('data', data => {
			fs.appendFileSync(stdoutOutputPath, data);
		});

		ps.stderr.on('data', data => {
			fs.appendFileSync(stderrOutputPath, data);
		});

		return new Promise((resolve, reject) => {
			ps.on('error', err => {
				reject(err);
			});

			ps.on('close', code => {
				resolve(code);
			});
		});
	}

	private async compileModInternal(
		modSourcePath: string,
		targetDllName: string,
		bits: number,
		stdoutOutputPath: string,
		stderrOutputPath: string,
		modId: string,
		modVersion: string,
		extraArgs: string[],
		pchPath?: string
	): Promise<number | null> {
		const gppPath = path.join(this.compilerPath, 'bin', 'g++.exe');
		const engineLibPath = path.join(this.enginePath, bits.toString(), 'windhawk.lib');
		const compiledModPath = path.join(this.engineModsPath, bits.toString(), targetDllName);

		fs.mkdirSync(path.dirname(compiledModPath), { recursive: true });

		const cppVersion = [
			'chrome-ui-tweaks\n1.0.0',
			'taskbar-vertical\n1.0',
		].includes(`${modId}\n${modVersion}`) ? 20 : 23;

		const windowsVersionFlags = [
			'aerexplorer\n1.6.2',
			'classic-taskdlg-fix\n1.1.0',
			'msg-box-font-fix\n1.5.0',
		].includes(`${modId}\n${modVersion}`) ? [] : [
			'-DWINVER=0x0A00',
			'-D_WIN32_WINNT=0x0A00',
			'-D_WIN32_IE=0x0A00',
			'-DNTDDI_VERSION=0x0A000008',
		];

		let backwardCompatibilityFlags: string[] = [];

		if ([
			'accent-color-sync\n1.31',
			'aerexplorer\n1.6.2',
			'basic-themer\n1.1.0',
			'classic-maximized-windows-fix\n2.1',
			'taskbar-vertical\n1.0',
			'win7-alttab-loader\n1.0.2',
			'ce-disable-process-button-flashing\n1.0.1',
			'msg-box-font-fix\n1.5.0',
			'sib-plusplus-tweaker\n0.7',
			'windows-7-clock-spacing\n1.0.0',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-DWH_ENABLE_DEPRECATED_PARTS');
		}

		if ([
			'classic-explorer-treeview\n1.1',
			'taskbar-button-scroll\n1.0.6',
			'taskbar-clock-customization\n1.3.3',
			'taskbar-notification-icon-spacing\n1.0.2',
			'taskbar-vertical\n1.0',
			'taskbar-wheel-cycle\n1.1.3',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-lruntimeobject');
		}

		if ([
			'taskbar-empty-space-clicks\n1.3',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-DUIATYPES_H');
		}

		const args = [
			`-std=c++${cppVersion}`,
			'-O2',
			'-shared',
			'-DUNICODE',
			'-D_UNICODE',
			...windowsVersionFlags,
			'-D__USE_MINGW_ANSI_STDIO=0',
			'-DWH_MOD',
			'-DWH_MOD_ID=L"' + modId.replace(/"/g, '\\"') + '"',
			'-DWH_MOD_VERSION=L"' + modVersion.replace(/"/g, '\\"') + '"',
			engineLibPath,
			modSourcePath,
			'-include',
			'windhawk_api.h',
			'-target',
			bits === 64 ? 'x86_64-w64-mingw32' : 'i686-w64-mingw32',
			'-o',
			compiledModPath,
			...(pchPath ? ['-include-pch', pchPath] : []),
			...extraArgs,
			...backwardCompatibilityFlags,
		];
		const ps = child_process.spawn(gppPath, args, {
			cwd: this.compilerPath
		});

		fs.writeFileSync(stdoutOutputPath, '');
		fs.writeFileSync(stderrOutputPath, '');

		ps.stdout.on('data', data => {
			fs.appendFileSync(stdoutOutputPath, data);
		});

		ps.stderr.on('data', data => {
			fs.appendFileSync(stderrOutputPath, data);
		});

		return new Promise((resolve, reject) => {
			ps.on('error', err => {
				reject(err);
			});

			ps.on('close', code => {
				resolve(code);
			});
		});
	}

	private deleteOldModFilesInFolder(modId: string, bits: number, currentDllName?: string) {
		const compiledModsPath = path.join(this.engineModsPath, bits.toString());

		let compiledModsDir: fs.Dir;
		try {
			compiledModsDir = fs.opendirSync(compiledModsPath);
		} catch (e) {
			// Ignore if file doesn't exist.
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
					// Ignore errors.
				}
			}
		} finally {
			compiledModsDir.closeSync();
		}
	}

	private deleteOldModFiles(modId: string, currentDllName?: string) {
		this.deleteOldModFilesInFolder(modId, 32, currentDllName);
		this.deleteOldModFilesInFolder(modId, 64, currentDllName);
	}

	private copyCompilerLibs(bits: number) {
		const llvmArch = bits === 64 ? 'x86_64-w64-mingw32' : 'i686-w64-mingw32';

		const libsPath = path.join(this.compilerPath, llvmArch, 'bin');
		const targetModsPath = path.join(this.engineModsPath, bits.toString());

		fs.mkdirSync(path.dirname(targetModsPath), { recursive: true });

		const libsDir = fs.opendirSync(libsPath);

		try {
			let libsDirEntry: fs.Dirent | null;
			while ((libsDirEntry = libsDir.readSync()) !== null) {
				if (!libsDirEntry.isFile()) {
					continue;
				}

				const filename = libsDirEntry.name;

				const libPath = path.join(libsPath, filename);
				const libPathDest = path.join(targetModsPath, filename);

				if (fs.existsSync(libPathDest) &&
					fs.statSync(libPathDest).mtimeMs === fs.statSync(libPath).mtimeMs) {
					continue;
				}

				try {
					fs.copyFileSync(libPath, libPathDest);
				} catch (e) {
					if (!fs.existsSync(libPathDest)) {
						throw e;
					}

					// The lib file already exists, perhaps it's in use.
					// Try to rename it to a temporary name.
					const libPathDestExt = path.extname(libPathDest);
					const libPathDestBaseName = path.basename(libPathDest, libPathDestExt);
					for (let i = 1; ; i++) {
						const tempFilename = libPathDestBaseName + '_temp' + i + libPathDestExt;
						const libPathDestTemp = path.join(targetModsPath, tempFilename);
						try {
							fs.renameSync(libPathDest, libPathDestTemp);
							break;
						} catch (e) {
							if (!fs.existsSync(libPathDestTemp)) {
								throw e;
							}
						}
					}

					fs.copyFileSync(libPath, libPathDest);
				}
			}
		} finally {
			libsDir.closeSync();
		}
	}

	public async compileMod(
		modId: string,
		modVersion: string,
		workspacePaths: WorkspacePaths,
		architecture?: string[],
		compilerOptions?: string
	) {
		let targetDllName: string;
		for (; ;) {
			targetDllName = modId + '_' + modVersion + '_' + randomIntFromInterval(100000, 999999) + '.dll';
			if (!this.doesCompiledModExist(targetDllName, 32) &&
				!this.doesCompiledModExist(targetDllName, 64)) {
				break;
			}
		}

		let compilerOptionsArray: string[] = [];
		if (compilerOptions && compilerOptions.trim() !== '') {
			compilerOptionsArray = splitargs(compilerOptions);
		}

		const allArchitectures: { [key: string]: number } = {
			'x86': 32,
			'x86-64': 64
		};

		for (const arch of architecture || Object.keys(allArchitectures)) {
			const bits = allArchitectures[arch];
			if (!bits) {
				throw new Error('Unknown architecture: ' + arch);
			}

			let pchPath: string | undefined = undefined;
			const pchHeaderPath = path.join(path.dirname(workspacePaths.modSourcePath), 'windhawk_pch.h');
			if (fs.existsSync(pchHeaderPath)) {
				pchPath = path.join(path.dirname(workspacePaths.modSourcePath), `windhawk_${arch}.pch`);
				if (!fs.existsSync(pchPath) ||
					fs.statSync(pchPath).mtimeMs < fs.statSync(pchHeaderPath).mtimeMs) {
					const result = await this.makePrecompiledHeaders(
						pchHeaderPath,
						pchPath,
						bits,
						workspacePaths.stdoutOutputPath,
						workspacePaths.stderrOutputPath,
						modId,
						modVersion,
						compilerOptionsArray
					);
					if (result !== 0) {
						throw new CompilerError(
							workspacePaths.stdoutOutputPath,
							workspacePaths.stderrOutputPath
						);
					}
				}
			}

			const result = await this.compileModInternal(
				workspacePaths.modSourcePath,
				targetDllName,
				bits,
				workspacePaths.stdoutOutputPath,
				workspacePaths.stderrOutputPath,
				modId,
				modVersion,
				compilerOptionsArray,
				pchPath
			);
			if (result !== 0) {
				throw new CompilerError(
					workspacePaths.stdoutOutputPath,
					workspacePaths.stderrOutputPath
				);
			}

			this.copyCompilerLibs(bits);
		}

		return {
			targetDllName,
			deleteOldModFiles: () => this.deleteOldModFiles(modId, targetDllName)
		};
	}

	public deleteModFiles(modId: string) {
		this.deleteOldModFiles(modId);
	}
}

// https://stackoverflow.com/a/7228322
// min and max included
function randomIntFromInterval(min: number, max: number) {
	return Math.floor(Math.random() * (max - min + 1) + min);
}

// https://github.com/elgs/splitargs
function splitargs(input: string, sep?: RegExp, keepQuotes?: boolean) {
	const separator = sep || /\s/g;
	let singleQuoteOpen = false;
	let doubleQuoteOpen = false;
	let tokenBuffer = [];
	const ret = [];

	const arr = input.split('');
	for (let i = 0; i < arr.length; ++i) {
		const element = arr[i];
		const matches = element.match(separator);
		if (element === "'" && !doubleQuoteOpen) {
			if (keepQuotes === true) {
				tokenBuffer.push(element);
			}
			singleQuoteOpen = !singleQuoteOpen;
			continue;
		} else if (element === '"' && !singleQuoteOpen) {
			if (keepQuotes === true) {
				tokenBuffer.push(element);
			}
			doubleQuoteOpen = !doubleQuoteOpen;
			continue;
		}

		if (!singleQuoteOpen && !doubleQuoteOpen && matches) {
			if (tokenBuffer.length > 0) {
				ret.push(tokenBuffer.join(''));
				tokenBuffer = [];
			} else if (sep) {
				ret.push(element);
			}
		} else {
			tokenBuffer.push(element);
		}
	}
	if (tokenBuffer.length > 0) {
		ret.push(tokenBuffer.join(''));
	} else if (sep) {
		ret.push('');
	}
	return ret;
}
