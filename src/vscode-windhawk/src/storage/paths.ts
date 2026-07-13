import * as reg from 'native-reg';
import * as path from 'path';
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

// Split a "HIVE\sub\key" string into its hive handle and the remaining subkey.
// Accepts both the short (HKCU/HKU/HKLM) and long (HKEY_*) hive prefixes.
export function parseRegistryKey(registryKey: string): { hive: reg.HKEY, subKey: string } {
	let i = registryKey.indexOf('\\');
	if (i === -1) {
		i = registryKey.length;
	}

	let hive: reg.HKEY;
	switch (registryKey.slice(0, i)) {
		case 'HKEY_CURRENT_USER':
		case 'HKCU':
			hive = reg.HKCU;
			break;

		case 'HKEY_USERS':
		case 'HKU':
			hive = reg.HKU;
			break;

		case 'HKEY_LOCAL_MACHINE':
		case 'HKLM':
			hive = reg.HKLM;
			break;

		default:
			throw new Error('Unsupported registry path');
	}

	return {
		hive,
		subKey: registryKey.slice(i + 1)
	};
}

export function getStoragePaths(opts: { appRoot: string }): StoragePaths {
	const appRootPath = opts.appRoot;
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

	const { hive: regKey, subKey: regSubKey } = parseRegistryKey(storageConfig.Storage.RegistryKey);

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
