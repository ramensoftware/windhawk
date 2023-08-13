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

	private async compileModInternal(
		modSourcePath: string,
		targetDllName: string,
		bits: number,
		stdoutOutputPath: string,
		stderrOutputPath: string,
		modId: string,
		modVersion: string,
		extraArgs: string[]
	) : Promise<number | null> {
		const gppPath = path.join(this.compilerPath, 'bin', 'g++.exe');
		const engineLibPath = path.join(this.enginePath, bits.toString(), 'windhawk.lib');
		const compiledModPath = path.join(this.engineModsPath, bits.toString(), targetDllName);

		fs.mkdirSync(path.dirname(compiledModPath), { recursive: true });

		const args = [
			'-std=c++20',
			'-O2',
			'-shared',
			'-DUNICODE',
			'-D_UNICODE',
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
			...extraArgs
		];
		const ps = child_process.spawn(gppPath, args, {
			cwd: this.compilerPath
		});

		fs.writeFileSync(stdoutOutputPath, '');
		fs.writeFileSync(stderrOutputPath, '');

		ps.stdout.on('data', data => {
			//console.log(`ps stdout: ${data}`);
			fs.appendFileSync(stdoutOutputPath, data);
		});

		ps.stderr.on('data', data => {
			//console.log(`ps stderr: ${data}`);
			fs.appendFileSync(stderrOutputPath, data);
		});

		return new Promise((resolve, reject) => {
			ps.on('error', err => {
				//console.log('Oh no, the error: ' + err);
				reject(err);
			});

			ps.on('close', code => {
				//console.log(`ps process exited with code ${code}`);
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
				if (!filenamePart.match(/^[0-9]+$/)) {
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
		for (;;) {
			targetDllName = modId + '_' + randomIntFromInterval(100000, 999999) + '.dll';
			if (!this.doesCompiledModExist(targetDllName, 32) &&
				!this.doesCompiledModExist(targetDllName, 64)) {
				break;
			}
		}

		let compilerOptionsArray: string[] = [];
		if (compilerOptions && compilerOptions.trim() !== '') {
			if (compilerOptions.includes('"')) {
				// Support can be added later, for now reject such input.
				throw new Error('Compiler options can\'t contain quotes');
			}

			compilerOptionsArray = compilerOptions.trim().split(/\s+/);
		}

		if (!architecture || architecture.includes('x86')) {
			const result = await this.compileModInternal(
				workspacePaths.modSourcePath,
				targetDllName,
				32,
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

			this.copyCompilerLibs(32);
		}

		if (!architecture || architecture.includes('x86-64')) {
			const result = await this.compileModInternal(
				workspacePaths.modSourcePath,
				targetDllName,
				64,
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

			this.copyCompilerLibs(64);
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
