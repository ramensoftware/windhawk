import { createWindhawkCore, Logger, WindhawkCore } from '../coreClient';
import { resolveAppRoot } from './appRoot';
import { createStderrLogger } from './logger';
import { readCliVersion, readRawWindhawkVersion } from './windhawkVersion';

export type GlobalOpts = {
	appRoot?: string;
	json: boolean;
	yes: boolean;
	quiet: boolean;
};

export type Environment = {
	core: WindhawkCore;
	globalOpts: GlobalOpts;
	logger: Logger;
};

// Resolve the Windhawk environment from CLI global options and construct the
// core the commands talk to. Throws EnvInvalidError (mapped to exit code 3)
// when the app root cannot be located.
export function loadEnvironment(opts: GlobalOpts): Environment {
	const appRoot = resolveAppRoot(opts.appRoot);
	const logger = createStderrLogger({ quiet: opts.quiet });
	const core = createWindhawkCore({
		appRoot,
		arm64Enabled: process.env['WINDHAWK_ARM64_ENABLED'] === '1',
		// The installed Windhawk version, read from the CLI's bundled
		// package.json - the same value (and source) the extension uses. Feeds
		// the compiler's WH_WINDHAWK_VERSION define and the precompiled-mod
		// minimum-version gate.
		windhawkVersion: readRawWindhawkVersion() ?? null,
		userAgentProduct: `windhawk-cli/${readCliVersion()}`,
		logger,
	});
	return { core, globalOpts: opts, logger };
}
