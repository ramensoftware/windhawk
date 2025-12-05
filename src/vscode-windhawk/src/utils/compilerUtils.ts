import * as child_process from 'child_process';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';

type CompilationTarget =
	| 'i686-w64-mingw32'
	| 'x86_64-w64-mingw32'
	| 'aarch64-w64-mingw32';

type CompilationResult = {
	exitCode: number | null;
	stdout: string;
	stderr: string;
};

export class CompilerError extends Error {
	public exitCode: number | null;
	public stdout: string;
	public stderr: string;

	constructor(target: CompilationTarget, result: number | null, stdout: string, stderr: string) {
		let msg = 'Compilation failed';

		if (result === 1) {
			msg += ', the mod might require a newer Windhawk version';
			if (target === 'aarch64-w64-mingw32') {
				msg += ', or perhaps the mod isn\'t compatible with ARM64 yet';
			}
		} else if (result === 0xC0000135) {
			msg += ', some files are missing, please reinstall Windhawk and ' +
				'make sure files aren\'t being removed by an antivirus';
		} else {
			const exitCodeStr = result !== null ? `0x${result.toString(16)}` : 'unknown';
			msg += `, error code: ${exitCodeStr}, please reinstall Windhawk ` +
				'and make sure files aren\'t being removed by an antivirus';
		}

		super(msg);
		this.exitCode = result;
		this.stdout = stdout;
		this.stderr = stderr;
	}
}

export default class CompilerUtils {
	private compilerPath: string;
	private enginePath: string;
	private engineModsPath: string;
	private arm64Enabled: boolean;
	private supportedCompilationTargets: CompilationTarget[];
	private activeProcesses: Set<child_process.ChildProcess> = new Set();

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

		for (const target of this.supportedCompilationTargets) {
			try {
				this.copyCompilerLibs(target);
			} catch (e: unknown) {
				const message = e instanceof Error ? e.message : String(e);
				vscode.window.showErrorMessage(`Failed to copy compiler libs for target ${target}: ${message}`);
			}
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
		modId: string,
		modVersion: string,
		extraArgs: string[],
	): Promise<CompilationResult> {
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

		this.activeProcesses.add(ps);

		const stdoutBuffers: Buffer[] = [];
		const stderrBuffers: Buffer[] = [];

		ps.stdout.on('data', data => {
			stdoutBuffers.push(data);
		});

		ps.stderr.on('data', data => {
			stderrBuffers.push(data);
		});

		return new Promise((resolve, reject) => {
			ps.on('error', err => {
				this.activeProcesses.delete(ps);
				reject(err);
			});

			ps.on('close', code => {
				this.activeProcesses.delete(ps);
				const stdout = Buffer.concat(stdoutBuffers).toString('utf8');
				const stderr = Buffer.concat(stderrBuffers).toString('utf8');
				resolve({ exitCode: code, stdout, stderr });
			});
		});
	}

	private async compileModInternal(
		modSourceCode: string,
		targetDllName: string,
		target: CompilationTarget,
		modId: string,
		modVersion: string,
		extraArgs: string[],
		pchPath?: string
	): Promise<CompilationResult> {
		const clangPath = path.join(this.compilerPath, 'bin', 'clang++.exe');

		const subfolder = this.subfolderFromCompilationTarget(target);
		const engineLibPath = path.join(this.enginePath, subfolder, 'windhawk.lib');
		const compiledModDllPath = path.join(this.engineModsPath, subfolder, targetDllName);

		fs.mkdirSync(path.dirname(compiledModDllPath), { recursive: true });

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
			'chrome-ui-tweaks\n1.0.0',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-include', 'atomic', '-include', 'optional');
		}

		if ([
			'sib-plusplus-tweaker\n0.7.1',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-include', 'atomic');
		}

		if ([
			'classic-explorer-treeview\n1.1.3',
			'sysdm-general-tab\n1.1',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-include', 'cmath');
		}

		if ([
			'ce-disable-process-button-flashing\n1.0.1',
			'windows-7-clock-spacing\n1.0.0',
		].includes(`${modId}\n${modVersion}`)) {
			backwardCompatibilityFlags.push('-include', 'vector');
		}

		const args = [
			'-std=c++23',
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
			'-x',
			'c++',
			'-',
			'-include',
			'windhawk_api.h',
			'-target',
			target,
			'-Wl,--export-all-symbols',
			'-o',
			compiledModDllPath,
			...(pchPath ? ['-include-pch', pchPath] : []),
			...extraArgs,
			...backwardCompatibilityFlags,
		];
		const ps = child_process.spawn(clangPath, args, {
			cwd: this.compilerPath
		});

		this.activeProcesses.add(ps);

		const stdoutBuffers: Buffer[] = [];
		const stderrBuffers: Buffer[] = [];

		ps.stdout.on('data', data => {
			stdoutBuffers.push(data);
		});

		ps.stderr.on('data', data => {
			stderrBuffers.push(data);
		});

		ps.stdin.write(modSourceCode);
		ps.stdin.end();

		return new Promise((resolve, reject) => {
			ps.on('error', err => {
				this.activeProcesses.delete(ps);
				reject(err);
			});

			ps.on('close', code => {
				this.activeProcesses.delete(ps);
				const stdout = Buffer.concat(stdoutBuffers).toString('utf8');
				const stderr = Buffer.concat(stderrBuffers).toString('utf8');
				resolve({ exitCode: code, stdout, stderr });
			});
		});
	}

	private copyCompilerLibs(target: CompilationTarget) {
		const libsDir = path.join(this.compilerPath, target, 'bin');
		const targetModsDir = path.join(this.engineModsPath, this.subfolderFromCompilationTarget(target));

		fs.mkdirSync(targetModsDir, { recursive: true });

		const filesToCopy = [
			['libc++.dll', 'libc++.whl'],
			['libunwind.dll', 'libunwind.whl'],
			['windhawk-mod-shim.dll', 'windhawk-mod-shim.dll'],
		];

		// Make sure libc++.dll from previous Windhawk versions is also
		// up-to-date to address the "Not enough space for thread data" error.
		if (fs.existsSync(path.join(targetModsDir, 'libc++.dll'))) {
			filesToCopy.push(['libc++.dll', 'libc++.dll']);
		}

		// Do the same for libunwind.dll.
		if (fs.existsSync(path.join(targetModsDir, 'libunwind.dll'))) {
			filesToCopy.push(['libunwind.dll', 'libunwind.dll']);
		}

		for (const [fileFrom, fileTo] of filesToCopy) {
			const libPath = path.join(libsDir, fileFrom);
			const libPathDest = path.join(targetModsDir, fileTo);

			if (fs.existsSync(libPathDest) &&
				fs.statSync(libPathDest).mtimeMs === fs.statSync(libPath).mtimeMs) {
				continue;
			}

			try {
				fs.copyFileSync(libPath, libPathDest);
			} catch (e: unknown) {
				if (!fs.existsSync(libPathDest)) {
					throw e;
				}

				// The lib file already exists, perhaps it's in use.
				// Try to rename it to a temporary name.
				const libPathDestExt = path.extname(libPathDest);
				const libPathDestBaseName = path.basename(libPathDest, libPathDestExt);
				for (let i = 1; ; i++) {
					const tempFilename = libPathDestBaseName + '_temp' + i + libPathDestExt;
					const libPathDestTemp = path.join(targetModsDir, tempFilename);
					try {
						fs.renameSync(libPathDest, libPathDestTemp);
						break;
					} catch (e: unknown) {
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
		modSourceCode: string,
		architectures: string[],
		compilerOptions: string | undefined,
		precompiledHeadersFolder?: string
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
			if (precompiledHeadersFolder) {
				const pchHeaderPath = path.join(precompiledHeadersFolder, 'windhawk_pch.h');
				if (fs.existsSync(pchHeaderPath)) {
					pchPath = path.join(precompiledHeadersFolder, `windhawk_t_${target}.pch`);
					if (!fs.existsSync(pchPath) ||
						fs.statSync(pchPath).mtimeMs < fs.statSync(pchHeaderPath).mtimeMs) {
						const { exitCode, stdout, stderr } = await this.makePrecompiledHeaders(
							pchHeaderPath,
							pchPath,
							target,
							modId,
							modVersion,
							compilerOptionsArray
						);
						if (exitCode !== 0) {
							throw new CompilerError(
								target,
								exitCode,
								stdout,
								stderr
							);
						}

						if (stdout) {
							console.log(`Precompiled headers stdout for target ${target}:\n${stdout}`);
						}
						if (stderr) {
							console.log(`Precompiled headers stderr for target ${target}:\n${stderr}`);
						}
					}
				}
			}

			const { exitCode, stdout, stderr } = await this.compileModInternal(
				modSourceCode,
				targetDllName,
				target,
				modId,
				modVersion,
				compilerOptionsArray,
				pchPath
			);
			if (exitCode !== 0) {
				throw new CompilerError(
					target,
					exitCode,
					stdout,
					stderr
				);
			}

			if (stdout) {
				console.log(`Compiler stdout for target ${target}:\n${stdout}`);
			}
			if (stderr) {
				console.log(`Compiler stderr for target ${target}:\n${stderr}`);
			}
		}

		return {
			targetDllName,
		};
	}

	public cancelCompilation() {
		for (const process of this.activeProcesses) {
			try {
				// Needed for Windows: https://stackoverflow.com/a/77421143
				process.stdout?.destroy();
				process.stdin?.destroy();
				process.stderr?.destroy();

				process.kill();
			} catch (e: unknown) {
				console.error('Failed to kill compilation process:', e);
			}
		}

		this.activeProcesses.clear();
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
