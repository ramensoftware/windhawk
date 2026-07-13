// Typed error base for the services layer.
//
// Each concrete subclass stores a stable `code` string that callers can
// branch on (via `instanceof` or by reading `.code`). The base accepts a
// plain string for the code so downstream consumers can freely define their
// own error categories by extending this class — the services layer does
// not enumerate every possible code.

export class WindhawkError extends Error {
	public readonly code: string;

	constructor(code: string, message: string) {
		super(message);
		this.code = code;
		this.name = new.target.name;
	}
}

// Thrown by repoClient (catalog/source/versions fetches), modFiles
// (precompiled DLL downloads), and update (Windhawk installer download) when
// an HTTP request returns !ok or fetch itself rejects.
export class RepoUnreachableError extends WindhawkError {
	public readonly cause?: unknown;

	constructor(message: string, cause?: unknown) {
		super('REPO_UNREACHABLE', message);
		this.cause = cause;
	}
}

// Thrown by repoClient when the repository returns 404 for a mod resource:
// the mod (or the requested version) is known-absent, as opposed to a
// transport failure. The CLI maps this to its own exit code (5).
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
