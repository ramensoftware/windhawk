// https://stackoverflow.com/a/45074641
declare const v8debug: any;
const debug = typeof v8debug === 'object'
            || /--debug|--inspect/.test(process.execArgv.join(' '));

export default {
	urls: {
		modsUrlRoot: 'https://mods.windhawk.net/',
		modsFolder: 'https://mods.windhawk.net/mods/',
	},
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
