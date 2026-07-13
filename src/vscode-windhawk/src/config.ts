// https://stackoverflow.com/a/45074641
declare const v8debug: any;
const debug = typeof v8debug === 'object'
            || /--debug|--inspect/.test(process.execArgv.join(' '));

// Repository URLs are no longer configured here: they live behind the core
// contract (src/services/repoClient.ts), including the
// WINDHAWK_DEBUG_MODS_URL override.

export default {
	debug: debug ? {
		reactProjectBuildPath: String.raw`C:\Windhawk-dev\vscode-windhawk-ui\dist\apps\vscode-windhawk-ui`,
		appRootPath: String.raw`C:\Windhawk-dev\Windhawk`,
		disableMinimalMode: true,
		disableEnvVarCheck: true,
	} : {
		reactProjectBuildPath: null,
		appRootPath: null,
		disableMinimalMode: false,
		disableEnvVarCheck: false,
	},
};
