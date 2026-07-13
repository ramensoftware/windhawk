import * as fs from 'fs';
import * as path from 'path';
import * as semver from 'semver';

// The CLI bundle (dist/cli.js) ships inside the Windhawk install alongside the
// VSCode extension and is built from this same package.json, so its `version`
// is the exact value the extension reads via
// vscode.extensions.getExtension('m417z.windhawk').packageJSON.version - i.e.
// the installed Windhawk version. Reading it here keeps the CLI and the
// extension on a single definition of "current Windhawk version" instead of
// inferring it from the engine directory layout.
//
// __dirname is the bundle's dist/ folder, so '..' is the extension root. This
// is the same lookup index.ts already uses for `--version`.
//
// Cached: the bundled package.json can't change mid-process, and this is read
// per repo request (for the User-Agent) as well as at startup.
let cachedVersion: { value: string | undefined } | null = null;

function readPackageVersion(): string | undefined {
	if (!cachedVersion) {
		let value: string | undefined;
		try {
			const pkg = JSON.parse(
				fs.readFileSync(path.resolve(__dirname, '..', 'package.json'), 'utf8'),
			) as { version?: string };
			value = pkg.version;
		} catch {
			// Bundled package.json missing or unreadable: degrade gracefully so
			// --version and the User-Agent don't crash a real operation.
			value = undefined;
		}
		cachedVersion = { value };
	}
	return cachedVersion.value;
}

// Raw version string for `windhawk-cli --version`. Falls back to 0.0.0 only if
// the bundled package.json is somehow missing a version.
export function readCliVersion(): string {
	return readPackageVersion() ?? '0.0.0';
}

// Raw installed-Windhawk version string for the core session (which coerces
// it internally where comparisons are needed). Unlike readCliVersion there
// is no fallback: an unknown version must stay unknown so version gates
// disable themselves instead of comparing against 0.0.0.
export function readRawWindhawkVersion(): string | undefined {
	return readPackageVersion();
}

// Coerced installed-Windhawk SemVer for the services layer (the compiler's
// WH_WINDHAWK_VERSION define and the precompiled-mod minimum-version gate) and
// the `update` command. Null only if the version string is missing or
// unparsable, in which case those consumers fall back to their version-0 /
// no-gate behavior.
export function getInstalledWindhawkVersion(): semver.SemVer | null {
	return semver.coerce(readPackageVersion());
}
