import * as child_process from 'child_process';
import * as fs from 'fs';
import * as path from 'path';

type WorkspacePaths = {
	modSourcePath: string,
	stdoutOutputPath: string,
	stderrOutputPath: string
};

type CompilationTarget =
	| 'i686-w64-mingw32'
	| 'x86_64-w64-mingw32'
	| 'aarch64-w64-mingw32';

export class CompilerError extends Error {
	public stdoutPath: string;
	public stderrPath: string;

	constructor(target: CompilationTarget, result: number | null, stdoutPath: string, stderrPath: string) {
		let msg = 'Compilation failed';

		if (result === 1) {
			msg = ', the mod might require a newer Windhawk version';
			if (target === 'aarch64-w64-mingw32') {
				msg += ', or perhaps the mod isn\'t compatible with ARM64 yet';
			}
		} else if (result === 0xC0000135) {
			msg = ', some files are missing, please reinstall Windhawk and ' +
				'make sure files aren\'t being removed by an antivirus';
		} else {
			const codeStr = result?.toString(16) ?? 'unknown';
			msg = `, error code: ${codeStr}, please reinstall Windhawk and ` +
				'make sure files aren\'t being removed by an antivirus';
		}

		super(msg);
		this.stdoutPath = stdoutPath;
		this.stderrPath = stderrPath;
	}
}

export default class CompilerUtils {
	private compilerPath: string;
	private enginePath: string;
	private engineModsPath: string;
	private arm64Enabled: boolean;
	private supportedCompilationTargets: CompilationTarget[];

	public constructor(compilerPath: string, enginePath: string, appDataPath: string, arm64Enabled: boolean) {
		this.compilerPath = compilerPath;
		this.enginePath = enginePath;
		this.engineModsPath = path.join(appDataPath, 'Engine', 'Mods');
		this.arm64Enabled = arm64Enabled;

		this.supportedCompilationTargets = [
			'i686-w64-mingw32',
			'x86_64-w64-mingw32',
		];

		if (arm64Enabled) {
			this.supportedCompilationTargets.push('aarch64-w64-mingw32');
		}
	}

	private subfolderFromCompilationTarget(target: CompilationTarget) {
		switch (target) {
			case 'i686-w64-mingw32':
				return '32';

			case 'x86_64-w64-mingw32':
				return '64';

			case 'aarch64-w64-mingw32':
				return 'arm64';
		}
	}

	private compilationTargetsFromArchitecture(architectures: string[], modTargets: string[]) {
		if (architectures.length === 0) {
			architectures = ['x86', 'x86-64'];
		}

		// Keep in lowercase.
		const commonSystemModTargets = [
			'startmenuexperiencehost.exe',
			'searchhost.exe',
			'explorer.exe',
			'shellexperiencehost.exe',
			'shellhost.exe',
			'dwm.exe',
			'notepad.exe',
			'regedit.exe'
		];

		const targets: CompilationTarget[] = [];

		for (const architecture of architectures) {
			if (architecture === 'x86') {
				targets.push('i686-w64-mingw32');
				continue;
			}

			if (architecture === 'x86-64') {
				if (this.arm64Enabled) {
					targets.push('aarch64-w64-mingw32');
					if (modTargets.length == 0 ||
						!modTargets.every(target => commonSystemModTargets.includes(target.toLowerCase()))) {
						targets.push('x86_64-w64-mingw32');
					}
				} else {
					targets.push('x86_64-w64-mingw32');
				}
				continue;
			}

			if (architecture === 'amd64') {
				targets.push('x86_64-w64-mingw32');
				continue;
			}

			if (architecture === 'arm64') {
				if (this.arm64Enabled) {
					targets.push('aarch64-w64-mingw32');
				}
				continue;
			}

			throw new Error(`Unsupported architecture: ${architecture}`);
		}

		if (targets.length === 0) {
			throw new Error('The current architecture is not supported');
		}

		return targets;
	}

	private doesCompiledModExist(fileName: string, target: CompilationTarget) {
		const compiledModPath = path.join(this.engineModsPath, this.subfolderFromCompilationTarget(target), fileName);
		return fs.existsSync(compiledModPath);
	}

	private async makePrecompiledHeaders(
		pchHeaderPath: string,
		targetPchPath: string,
		target: CompilationTarget,
		stdoutOutputPath: string,
		stderrOutputPath: string,
		modId: string,
		modVersion: string,
		extraArgs: string[],
	): Promise<number | null> {
		const clangPath = path.join(this.compilerPath, 'bin', 'clang++.exe');

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
			target,
			'-o',
			targetPchPath,
			...extraArgs.filter(arg => arg.startsWith('-D'))
		];
		const ps = child_process.spawn(clangPath, args, {
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
		target: CompilationTarget,
		stdoutOutputPath: string,
		stderrOutputPath: string,
		modId: string,
		modVersion: string,
		extraArgs: string[],
		pchPath?: string
	): Promise<number | null> {
		const clangPath = path.join(this.compilerPath, 'bin', 'clang++.exe');

		const subfolder = this.subfolderFromCompilationTarget(target);
		const engineLibPath = path.join(this.enginePath, subfolder, 'windhawk.lib');
		const compiledModPath = path.join(this.engineModsPath, subfolder, targetDllName);

		fs.mkdirSync(path.dirname(compiledModPath), { recursive: true });

		const windowsVersionFlags = [
			'classic-taskdlg-fix\n1.1.0',
		].includes(`${modId}\n${modVersion}`) ? [] : [
			'-DWINVER=0x0A00',
			'-D_WIN32_WINNT=0x0A00',
			'-D_WIN32_IE=0x0A00',
			'-DNTDDI_VERSION=0x0A000008',
		];

		const backwardCompatibilityFlags: string[] = [];

		if ([
			'classic-maximized-windows-fix\n2.1',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-DWH_ENABLE_DEPRECATED_PARTS');
		}

		if ([
			'alt-tab-delayer\n1.1.0',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-include', 'atomic');
		}

		if ([
			'chrome-ui-tweaks\n1.0.0',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-include', 'atomic', '-include', 'optional');
		}

		if ([
			'classic-explorer-treeview\n1.1.3',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-include', 'cmath');
		}

		if ([
			'sib-plusplus-tweaker\n0.7.1',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-include', 'atomic');
		}

		if ([
			'sysdm-general-tab\n1.1',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-include', 'cmath');
		}

		if ([
			'basic-themer\n1.1.0',
			'ce-disable-process-button-flashing\n1.0.1',
			'windows-7-clock-spacing\n1.0.0',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-include', 'vector');
		}

		const args = [
			`-std=c++23`,
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
			target,
			'-Wl,--export-all-symbols',
			'-o',
			compiledModPath,
			...(pchPath ? ['-include-pch', pchPath] : []),
			...extraArgs,
			...backwardCompatibilityFlags,
		];
		const ps = child_process.spawn(clangPath, args, {
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

	private deleteOldModFilesInFolder(modId: string, target: CompilationTarget, currentDllName?: string) {
		const compiledModsPath = path.join(this.engineModsPath, this.subfolderFromCompilationTarget(target));

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
		for (const target of this.supportedCompilationTargets) {
			this.deleteOldModFilesInFolder(modId, target, currentDllName);
		}
	}

	private copyCompilerLibs(target: CompilationTarget) {
		const libsPath = path.join(this.compilerPath, target, 'bin');
		const targetModsPath = path.join(this.engineModsPath, this.subfolderFromCompilationTarget(target));

		fs.mkdirSync(path.dirname(targetModsPath), { recursive: true });

		const filesToCopy = [
			['libc++.dll', 'libc++.whl'],
			['libunwind.dll', 'libunwind.whl'],
			['windhawk-mod-shim.dll', 'windhawk-mod-shim.dll'],
		];

		// Make sure libc++.dll from previous Windhawk versions is also
		// up-to-date to address the "Not enough space for thread data" error.
		if (fs.existsSync(path.join(targetModsPath, 'libc++.dll'))) {
			filesToCopy.push(['libc++.dll', 'libc++.dll']);
		}

		// Do the same for libunwind.dll.
		if (fs.existsSync(path.join(targetModsPath, 'libunwind.dll'))) {
			filesToCopy.push(['libunwind.dll', 'libunwind.dll']);
		}

		for (const [fileFrom, fileTo] of filesToCopy) {
			const libPath = path.join(libsPath, fileFrom);
			const libPathDest = path.join(targetModsPath, fileTo);

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
	}

	public async compileMod(
		modId: string,
		modVersion: string,
		modTargets: string[],
		workspacePaths: WorkspacePaths,
		architectures: string[],
		compilerOptions?: string
	) {
		let targetDllName: string;
		for (; ;) {
			targetDllName = modId + '_' + modVersion + '_' + randomIntFromInterval(100000, 999999) + '.dll';
			if (this.supportedCompilationTargets.every(target => !this.doesCompiledModExist(targetDllName, target))) {
				break;
			}
		}

		let compilerOptionsArray: string[] = [];
		if (compilerOptions && compilerOptions.trim() !== '') {
			compilerOptionsArray = splitargs(compilerOptions);
		}

		for (const target of this.compilationTargetsFromArchitecture(architectures, modTargets)) {
			let pchPath: string | undefined = undefined;
			const pchHeaderPath = path.join(path.dirname(workspacePaths.modSourcePath), 'windhawk_pch.h');
			if (fs.existsSync(pchHeaderPath)) {
				pchPath = path.join(path.dirname(workspacePaths.modSourcePath), `windhawk_${target}.pch`);
				if (!fs.existsSync(pchPath) ||
					fs.statSync(pchPath).mtimeMs < fs.statSync(pchHeaderPath).mtimeMs) {
					const result = await this.makePrecompiledHeaders(
						pchHeaderPath,
						pchPath,
						target,
						workspacePaths.stdoutOutputPath,
						workspacePaths.stderrOutputPath,
						modId,
						modVersion,
						compilerOptionsArray
					);
					if (result !== 0) {
						throw new CompilerError(
							target,
							result,
							workspacePaths.stdoutOutputPath,
							workspacePaths.stderrOutputPath
						);
					}
				}
			}

			const result = await this.compileModInternal(
				workspacePaths.modSourcePath,
				targetDllName,
				target,
				workspacePaths.stdoutOutputPath,
				workspacePaths.stderrOutputPath,
				modId,
				modVersion,
				compilerOptionsArray,
				pchPath
			);
			if (result !== 0) {
				throw new CompilerError(
					target,
					result,
					workspacePaths.stdoutOutputPath,
					workspacePaths.stderrOutputPath
				);
			}

			this.copyCompilerLibs(target);
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
