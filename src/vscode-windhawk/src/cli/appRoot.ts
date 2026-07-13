import * as fs from 'fs';
import * as path from 'path';
import { EnvInvalidError } from './errors';

// App-root DISCOVERY, extracted from environment.ts so the precedence logic is
// unit-testable on its own - importing environment.ts pulls in the whole
// core-client/service load chain (loadEnvironment calls createWindhawkCore),
// which this module deliberately does not. Discovery is host policy; the core
// only VALIDATES the resolved root (APP_ROOT_INVALID).

// Discovery precedence:
//   (1) explicit --app-root flag
//   (2) WINDHAWK_UI_PATH env var
//   (3) current working directory if it contains windhawk.ini
// Throws EnvInvalidError (the CLI maps it to exit code 3) when the root cannot
// be located.
export function resolveAppRoot(explicit: string | undefined): string {
	if (explicit) {
		if (!hasWindhawkIni(explicit)) {
			throw new EnvInvalidError(
				`--app-root path does not contain windhawk.ini: ${explicit}`,
			);
		}
		return explicit;
	}

	const uiPath = process.env['WINDHAWK_UI_PATH'];
	if (uiPath) {
		// If WINDHAWK_UI_PATH looks like a UI subdirectory (parent-of-parent
		// is the app root), use that.
		const derived = path.dirname(path.dirname(uiPath));
		if (hasWindhawkIni(derived)) {
			return derived;
		}
		// Otherwise treat WINDHAWK_UI_PATH itself as the app root.
		if (hasWindhawkIni(uiPath)) {
			return uiPath;
		}
	}

	if (hasWindhawkIni(process.cwd())) {
		return process.cwd();
	}

	throw new EnvInvalidError(
		'Could not locate Windhawk app root. Pass --app-root <path>, set WINDHAWK_UI_PATH, or run from the Windhawk installation directory.',
	);
}

export function hasWindhawkIni(dir: string): boolean {
	try {
		return fs.existsSync(path.join(dir, 'windhawk.ini'));
	} catch {
		return false;
	}
}
