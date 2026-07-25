// Typed error classes of the windhawk-core contract.
//
// Each concrete subclass stores a stable `code` string that callers branch on
// (via `instanceof` or by reading `.code`) - in particular the CLI-style
// exit-code mapping, which only consults `.code` when the error is
// `instanceof WindhawkError`. The DLL backend rebuilds these same classes from
// the wire error envelope (dllBackend.ts), so the boundary is transparent: a
// front-end catches the very class the contract documents regardless of how
// the failure was produced.

export class WindhawkError extends Error {
	public readonly code: string;

	constructor(code: string, message: string) {
		super(message);
		this.code = code;
		this.name = new.target.name;
	}
}

// Thrown for repository (catalog/source/versions), precompiled-DLL download,
// and Windhawk-installer download failures when the request returns !ok or the
// transport itself fails.
export class RepoUnreachableError extends WindhawkError {
	public readonly cause?: unknown;

	constructor(message: string, cause?: unknown) {
		super('REPO_UNREACHABLE', message);
		this.cause = cause;
	}
}

// Thrown when the repository returns 404 for a mod resource: the mod (or the
// requested version) is known-absent, as opposed to a transport failure. The
// CLI-style mapping routes this to its own exit code.
export class ModNotInRepoError extends WindhawkError {
	public readonly modId: string;
	public readonly version?: string;

	constructor(modId: string, version?: string, message?: string) {
		super(
			'MOD_NOT_IN_REPO',
			message ?? (version
				? `Version ${version} of mod '${modId}' is not available in the repository`
				: `Mod '${modId}' is not available in the repository`),
		);
		this.modId = modId;
		this.version = version;
	}
}

// The clang target triples a compile can run for.
export type CompilationTarget =
	| 'i686-w64-mingw32'
	| 'x86_64-w64-mingw32'
	| 'aarch64-w64-mingw32';

// A failed compile. The DLL backend reconstructs it from the COMPILER_FAILED
// wire details (target / exitCode / stdout / stderr); the constructor
// reproduces the same user-facing message the core sends.
export class CompilerError extends WindhawkError {
	public target: CompilationTarget;
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

		super('COMPILE_FAILED', msg);
		this.target = target;
		this.exitCode = result;
		this.stdout = stdout;
		this.stderr = stderr;
	}
}

// A compile aborted by cancellation.
export class CompilerKilled extends WindhawkError {
	constructor() {
		super('CANCELLED', 'Compilation was aborted');
	}
}
