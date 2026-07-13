import * as semver from 'semver';
import { createServices } from '../services';
import { Logger } from '../services/logger';
import { getStoragePaths } from '../storage/paths';
import { WindhawkCore } from './contract';
import { createDllBackend } from './dllBackend';
import { createInProcessBackend } from './inProcessBackend';
import { resolveRoutingFromEnv, selectCore } from './routing';

// Entry point of the core client: the only module the front-ends use to
// obtain a WindhawkCore. Commands route per the routing table
// (routing.ts): served by windhawk-core.dll when flagged and its
// artifacts load, by the in-process TypeScript backend otherwise. The
// dual-run diff mode (debug builds) can execute read-only routed commands
// on both backends and log result diffs.
//
// Loading this module pulls in the storage layer's native modules
// (native-reg, fs-ext, ini-win); callers that must stay light (the CLI's
// --help fast path) defer importing it, as they previously deferred the
// service bootstrap.

export * from './contract';
export { CoreDllError } from './dllBackend';

export type WindhawkCoreOptions = {
	// The Windhawk app root (the directory containing windhawk.ini).
	// DISCOVERY of the root is host policy and stays in the front-ends
	// (vscode.env.appRoot derivation, --app-root / WINDHAWK_UI_PATH / cwd);
	// validation and [Storage] resolution happen here.
	appRoot: string;
	arm64Enabled: boolean;
	// Raw installed-Windhawk version string (the extension's packageJSON
	// version / the CLI's bundled package.json); null when unknown. Coerced
	// internally where comparisons need it.
	windhawkVersion: string | null;
	// Product identity for the repository User-Agent, e.g. "Windhawk/1.7.3"
	// or "windhawk-cli/1.7.3". The " (portable)" suffix is appended
	// internally for portable installs.
	userAgentProduct: string;
	// Sink for user-facing messages (VSCode notifications / CLI stderr).
	logger: Logger;
};

// Build the routed core. Throws if appRoot does not contain a readable
// windhawk.ini (same error surface as the previous direct getStoragePaths
// call). A missing or unloadable windhawk-core.dll never fails creation:
// every command falls back to the in-process backend.
export function createWindhawkCore(options: WindhawkCoreOptions): WindhawkCore {
	const { appRoot, arm64Enabled, windhawkVersion, userAgentProduct, logger } = options;

	const storagePaths = getStoragePaths({ appRoot });
	const services = createServices({
		storagePaths,
		logger,
		arm64Enabled,
		// Coerced once here; feeds the compiler's WH_WINDHAWK_VERSION define
		// (which reads only major/minor/patch) and the precompiled-mod
		// minimum-version gate. The pre-release tag is kept so the gate orders
		// e.g. 2.0.0-alpha.1 below a 2.0.0 requirement.
		currentWindhawkVersion: semver.coerce(windhawkVersion, { includePrerelease: true }),
		userAgentProduct,
	});

	const inProcess = createInProcessBackend({
		services,
		env: {
			portable: storagePaths.portable,
			arm64Enabled,
			windhawkVersion,
			fsPaths: storagePaths.fsPaths,
		},
	});

	const routing = resolveRoutingFromEnv();
	return selectCore(
		inProcess,
		() =>
			createDllBackend({
				appRoot,
				arm64Enabled,
				windhawkVersion,
				// The full repository User-Agent, composed exactly like the
				// in-process RepoClient's (services/index.ts): product identity
				// plus the " (portable)" suffix for portable installs.
				userAgent: `${userAgentProduct}${storagePaths.portable ? ' (portable)' : ''}`,
				logger,
			}),
		routing,
		logger,
	);
}
