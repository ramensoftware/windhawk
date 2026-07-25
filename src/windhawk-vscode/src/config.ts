// https://stackoverflow.com/a/45074641
declare const v8debug: any;
const debug = typeof v8debug === 'object'
            || /--debug|--inspect/.test(process.execArgv.join(' '));

// Repository URLs are not configured here: they are core-internal (resolved
// inside windhawk-core.dll), including the WINDHAWK_DEBUG_MODS_URL override.

export default {
	debug: debug ? {
		reactProjectBuildPath: String.raw`C:\Windhawk-dev\windhawk-frontend\dist\apps\windhawk-frontend`,
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
