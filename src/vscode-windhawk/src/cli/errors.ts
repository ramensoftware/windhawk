import { WindhawkError } from '../coreClient/contract';

// CLI-layer errors.
//
// Errors in this file are thrown only by CLI code (command handlers and
// environment discovery). They extend WindhawkError so the output adapter
// can branch on `instanceof WindhawkError` and read `.code` uniformly with
// any error the core throws.
//
// Exit codes for each `code` value live in cli/output.ts's
// EXIT_CODE_BY_ERROR_CODE map.

// Input/validation failures at the CLI boundary (bad flag, missing required
// arg, unknown field, wrong value type). Exit code 2.
export class UsageError extends Error {
	constructor(message: string) {
		super(message);
		this.name = 'UsageError';
	}
}

// App-root discovery failed: no --app-root, no WINDHAWK_UI_PATH, no
// windhawk.ini in cwd. Exit code 3.
export class EnvInvalidError extends WindhawkError {
	constructor(message: string) {
		super('ENV_INVALID', message);
	}
}

// Command references a mod that isn't installed locally. Exit code 4. Thrown
// by CLI handlers after the core returns null from getModConfig (or rejects
// getModSource with ENOENT); the core itself indicates absence by returning
// null. (ModNotInRepoError, by contrast, is thrown by the core's repository
// client and is part of the contract.)
export class ModNotInstalledError extends WindhawkError {
	public readonly modId: string;

	constructor(modId: string, message?: string) {
		super('MOD_NOT_INSTALLED', message ?? `Mod not installed: ${modId}`);
		this.modId = modId;
	}
}

// App-settings change requires a Windhawk restart and the user did not pass
// --confirm-app-restart. CLI refuses to write. Exit code 8. Never thrown by
// services; the extension's updateAppSettings handler has no equivalent
// gate (it just calls postAppRestartBg unconditionally when the change
// demands it).
export class RestartRequiredError extends WindhawkError {
	constructor(message: string) {
		super('RESTART_REQUIRED', message);
	}
}
