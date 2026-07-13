import { CompilerError, WindhawkError } from '../coreClient/contract';
import { UsageError } from './errors';

// The --json output envelope. schemaVersion is locked to 1; changes that break
// backward compatibility must bump it.
const SCHEMA_VERSION = 1;

// The full set of error codes this CLI recognizes and the exit code each maps
// to. Includes codes thrown by CLI-layer errors (see cli/errors.ts), by the
// in-process backend (the WindhawkError subclasses re-exported from
// src/coreClient/contract.ts), and by windhawk-core.dll (the DLL wire codes,
// surfaced as a CoreDllError - itself a WindhawkError). A WindhawkError whose
// `code` is not listed here falls through to the GENERIC exit code - add an
// entry below when introducing a new error category.
//
// Some DLL wire codes are spelled differently from the CLI-layer/in-process
// names for the SAME condition; both spellings are listed and map to the same
// exit code, so a DLL-surfaced failure classifies identically to the in-process
// one:
//   ENV_INVALID    == APP_ROOT_INVALID  -> 3
//   COMPILE_FAILED == COMPILER_FAILED   -> 7
//   CANCELLED      == CANCELED          -> 9
// MOD_NOT_INSTALLED / MOD_NOT_IN_REPO / REPO_UNREACHABLE share their spelling
// across both sides. INVALID_REQUEST / IO_FAILED / REGISTRY_FAILED / INTERNAL /
// UPDATE_IN_PROGRESS are intentionally absent: they map to GENERIC (exit 1).
const EXIT_CODE_BY_ERROR_CODE: Record<string, number> = {
	ENV_INVALID: 3,
	APP_ROOT_INVALID: 3,
	MOD_NOT_INSTALLED: 4,
	MOD_NOT_IN_REPO: 5,
	REPO_UNREACHABLE: 6,
	COMPILE_FAILED: 7,
	COMPILER_FAILED: 7,
	RESTART_REQUIRED: 8,
	CANCELLED: 9,
	CANCELED: 9,
};

export interface Output {
	// Emit a successful result. In --json mode, wraps `data` in the envelope
	// and writes to stdout. In text mode, calls formatText() which is expected
	// to write directly to stdout.
	result<T>(data: T, formatText: () => void): void;

	// Emit an error and exit with the mapped code. In --json mode, writes the
	// error envelope to stdout. In text mode, writes "error: <message>" to
	// stderr. Never returns.
	error(err: unknown): never;
}

export function createOutput(json: boolean): Output {
	return {
		result(data, formatText) {
			if (json) {
				process.stdout.write(JSON.stringify({
					schemaVersion: SCHEMA_VERSION,
					success: true,
					data,
				}) + '\n');
			} else {
				formatText();
			}
		},
		error(err) {
			// A compile failure carries the real compiler diagnostics on
			// stdout/stderr, but classifyError only surfaces the generic summary
			// message. Stream the diagnostics to stderr (both text and json
			// modes, so stdout stays clean) - mirrors the extension's
			// reportCompilerException and the `[compile:<arch>]` diagnostic lines.
			if (err instanceof CompilerError) {
				writeCompilerDiagnostics(err);
			}
			const { category, message, exitCode } = classifyError(err);
			if (json) {
				process.stdout.write(JSON.stringify({
					schemaVersion: SCHEMA_VERSION,
					success: false,
					error: { code: category, message },
				}) + '\n');
			} else {
				process.stderr.write(`error: ${message}\n`);
			}
			process.exit(exitCode);
		},
	};
}

// Exported for unit testing: maps any thrown value to its CLI category +
// message + process exit code. `error()` above is the side-effecting wrapper
// (writes the envelope / stderr line and calls process.exit).
export function classifyError(e: unknown): { category: string; message: string; exitCode: number } {
	if (e instanceof UsageError) {
		return { category: 'USAGE', message: e.message, exitCode: 2 };
	}
	if (e instanceof WindhawkError) {
		const exitCode = EXIT_CODE_BY_ERROR_CODE[e.code] ?? 1;
		return { category: e.code, message: e.message, exitCode };
	}
	const message = e instanceof Error ? e.message : String(e);
	return { category: 'GENERIC', message, exitCode: 1 };
}

// Friendly architecture label for a compilation target, matching the labels
// used by the `Compiling for <arch>...` progress lines.
const ARCH_LABEL_BY_TARGET: Record<string, string> = {
	'i686-w64-mingw32': 'x86',
	'x86_64-w64-mingw32': 'x86-64',
	'aarch64-w64-mingw32': 'arm64',
};

// Write a failed compile's captured output to stderr, one `[compile:<arch>]`
// prefixed line at a time. The block-selection logic mirrors the extension's
// reportCompilerException: fall back to the raw exit code when there's no
// captured output or the failure isn't the usual exit-code-1 compile error.
function writeCompilerDiagnostics(err: CompilerError): void {
	const arch = ARCH_LABEL_BY_TARGET[err.target] ?? err.target;
	const stdout = err.stdout.trim();
	const stderr = err.stderr.trim();

	const blocks: string[] = [];
	if ((stdout === '' && stderr === '') || err.exitCode !== 1) {
		const exitCodeStr = err.exitCode !== null ? `0x${err.exitCode.toString(16)}` : 'unknown';
		blocks.push(`Exit code: ${exitCodeStr}`);
	}
	if (stdout !== '') {
		blocks.push(stdout);
	}
	if (stderr !== '') {
		blocks.push(stderr);
	}

	for (const line of blocks.join('\n').split('\n')) {
		process.stderr.write(`[compile:${arch}] ${line}\n`);
	}
}
