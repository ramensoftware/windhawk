import { WindhawkCore } from './contract';
import { createDllBackend } from './dllBackend';
import { Logger } from './logger';

// Entry point of the core client: the only module the front-end uses to obtain
// a WindhawkCore. Every command is served by windhawk-core.dll through the napi
// bridge (dllBackend.ts). There is no in-process backend: a missing or
// incompatible DLL is fatal and surfaces to the caller.

export * from './contract';
export { CoreDllError } from './dllBackend';

export type WindhawkCoreOptions = {
	// The Windhawk app root (the directory containing windhawk.ini).
	// DISCOVERY of the root is host policy and stays in the front-end
	// (vscode.env.appRoot derivation); the core validates it and resolves
	// [Storage] at session creation.
	appRoot: string;
	// Raw installed-Windhawk version string (the extension's packageJSON
	// version); null when unknown. The core coerces it internally where
	// comparisons need it, and builds the repository User-Agent from it.
	windhawkVersion: string | null;
	// Sink for user-facing messages (VSCode notifications / CLI stderr).
	logger: Logger;
};

// Build the DLL-backed core. Throws if the bridge/DLL cannot be loaded or the
// app root does not contain a readable windhawk.ini (the core maps that to
// APP_ROOT_INVALID at session creation).
export function createWindhawkCore(options: WindhawkCoreOptions): WindhawkCore {
	const { appRoot, windhawkVersion, logger } = options;

	// No userAgent override: the core composes the GUI-style default
	// ("Windhawk/<windhawkVersion>", plus " (portable)" for portable installs)
	// from the session config.
	return createDllBackend({ appRoot, windhawkVersion, logger }).commands;
}
