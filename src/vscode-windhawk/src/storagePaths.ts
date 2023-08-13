import * as reg from 'native-reg';
import * as path from 'path';
import * as vscode from 'vscode';
import config from './config';
import * as ini from './ini';

type FileSystemPaths = {
	appRootPath: string,
	appDataPath: string,
	enginePath: string,
	compilerPath: string,
	uiPath: string
};

type StoragePathsPortable = {
	portable: true,
	fsPaths: FileSystemPaths
};

type StoragePathsNonPortable = {
	portable: false,
	fsPaths: FileSystemPaths,
	regKey: reg.HKEY,
	regSubKey: string
};

export type StoragePaths =
	| StoragePathsPortable
	| StoragePathsNonPortable;

function getAppRootPath() {
	const debugAppRootPath = config.debug.appRootPath;
	if (debugAppRootPath) {
		return debugAppRootPath;
	}

	const vscodeInstallPath = vscode.env.appRoot; // returns <vscode_dir>\resources\app
	return path.dirname(path.dirname(path.dirname(vscodeInstallPath)));
}

function getStorageConfig(appRootPath: string) {
	const iniFilePath = path.join(appRootPath, 'windhawk.ini');
	return ini.fromFile(iniFilePath);
}

function expandEnvironmentVariables(path: string) {
	// https://stackoverflow.com/a/21363956
	return path.replace(/%([^%]+)%/g, (original, matched) => {
		return process.env[matched] ?? original;
	});
}

export function getStoragePaths(): StoragePaths {
	const appRootPath = getAppRootPath();
	const storageConfig = getStorageConfig(appRootPath);

	const portable = !!parseInt(storageConfig.Storage.Portable, 10);

	const processPath = (p: string) => path.resolve(appRootPath, expandEnvironmentVariables(p));

	const appDataPath = processPath(storageConfig.Storage.AppDataPath);
	const enginePath = processPath(storageConfig.Storage.EnginePath);
	const compilerPath = processPath(storageConfig.Storage.CompilerPath);
	const uiPath = processPath(storageConfig.Storage.UIPath);

	if (portable) {
		return {
			portable,
			fsPaths: {
				appRootPath,
				appDataPath,
				enginePath,
				compilerPath,
				uiPath
			}
		};
	}

	const registryKey = storageConfig.Storage.RegistryKey;
	let i = registryKey.indexOf('\\');
	if (i === -1) {
		i = registryKey.length;
	}

	let regKey: reg.HKEY;
	switch (registryKey.slice(0, i)) {
		case 'HKEY_CURRENT_USER':
		case 'HKCU':
			regKey = reg.HKCU;
			break;

		case 'HKEY_USERS':
		case 'HKU':
			regKey = reg.HKU;
			break;

		case 'HKEY_LOCAL_MACHINE':
		case 'HKLM':
			regKey = reg.HKLM;
			break;

		default:
			throw new Error('Unsupported registry path');
	}

	const regSubKey = registryKey.slice(i + 1);

	return {
		portable,
		fsPaths: {
			appRootPath,
			appDataPath,
			enginePath,
			compilerPath,
			uiPath
		},
		regKey,
		regSubKey
	};
}
