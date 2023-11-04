import * as fs from 'fs';
import * as reg from 'native-reg';
import * as path from 'path';
import * as ini from '../ini';

function getSettingsChangeTime() {
	// Unix timestamp in seconds, limited to a positive signed 32-bit integer.
	return (Date.now() / 1000) & 0x7fffffff;
}

type ModConfig = {
	libraryFileName: string,
	disabled: boolean,
	loggingEnabled: boolean,
	debugLoggingEnabled: boolean,
	include: string[],
	exclude: string[],
	includeCustom: string[],
	excludeCustom: string[],
	includeExcludeCustomOnly: boolean,
	architecture: string[],
	version: string
};

type ModSettings = Record<string, string | number>;

function mergeModSettings(existingSettings: ModSettings, newSettings: ModSettings) {
	const getNamePrefix = (name: string) => name.split('.', 1)[0].replace(/\[\d+\]$/, '[0]');

	const existingNamePrefixes: Record<string, boolean> = {};
	for (const name of Object.keys(existingSettings)) {
		existingNamePrefixes[getNamePrefix(name)] = true;
	}

	const mergedSettings: ModSettings = { ...existingSettings };
	let existingSettingsChanged = false;

	for (const [name, value] of Object.entries(newSettings)) {
		if (!existingNamePrefixes[getNamePrefix(name)]) {
			mergedSettings[name] = value;
			existingSettingsChanged = true;
		}
	}

	return { mergedSettings, existingSettingsChanged };
}

export interface ModConfigUtils {
	getConfigOfInstalled(): Record<string, ModConfig>;
	doesConfigExist(modId: string): boolean;
	getModConfig(modId: string): ModConfig | null;
	setModConfig(modId: string, config: Partial<ModConfig>, initialSettings?: ModSettings): void;
	getModSettings(modId: string): ModSettings;
	setModSettings(modId: string, settings: ModSettings): void;
	enableMod(modId: string, enable: boolean): void;
	enableLogging(modId: string, enable: boolean): void;
	deleteMod(modId: string): void;
	changeModId(modIdFrom: string, modIdTo: string): void;
}

export class ModConfigUtilsPortable implements ModConfigUtils {
	private engineModsPath: string;
	private engineModsWritablePath: string;

	public constructor(appDataPath: string) {
		this.engineModsPath = path.join(appDataPath, 'Engine', 'Mods');
		this.engineModsWritablePath = path.join(appDataPath, 'Engine', 'ModsWritable');
	}

	private getModIniPath(modId: string) {
		return path.join(this.engineModsPath, modId + '.ini');
	}

	private getModWritableIniPath(modId: string) {
		return path.join(this.engineModsWritablePath, modId + '.ini');
	}

	public getConfigOfInstalled() {
		const mods: Record<string, ModConfig> = {};

		let engineModsDir: fs.Dir;
		try {
			engineModsDir = fs.opendirSync(this.engineModsPath);
		} catch (e) {
			// Ignore if file doesn't exist.
			if (e.code !== 'ENOENT') {
				throw e;
			}
			return mods;
		}

		try {
			let engineModsDirEntry: fs.Dirent | null;
			while ((engineModsDirEntry = engineModsDir.readSync()) !== null) {
				if (engineModsDirEntry.isFile() && engineModsDirEntry.name.endsWith('.ini')) {
					const engineModPath = path.join(this.engineModsPath, engineModsDirEntry.name);
					const modConfig = ini.fromFileOrDefault(engineModPath);

					if (!modConfig.Mod?.LibraryFileName) {
						continue;
					}

					const modId = engineModsDirEntry.name.slice(0, -'.ini'.length);
					mods[modId] = {
						libraryFileName: modConfig.Mod.LibraryFileName,
						disabled: !!parseInt(modConfig.Mod.Disabled ?? '0', 10),
						loggingEnabled: !!parseInt(modConfig.Mod.LoggingEnabled ?? '0', 10),
						debugLoggingEnabled: !!parseInt(modConfig.Mod.DebugLoggingEnabled ?? '0', 10),
						include: (modConfig.Mod.Include ?? '').split('|'),
						exclude: (modConfig.Mod.Exclude ?? '').split('|'),
						includeCustom: (modConfig.Mod.IncludeCustom ?? '').split('|'),
						excludeCustom: (modConfig.Mod.ExcludeCustom ?? '').split('|'),
						includeExcludeCustomOnly: !!parseInt(modConfig.Mod.IncludeExcludeCustomOnly ?? '0', 10),
						architecture: (modConfig.Mod.Architecture ?? '').split('|'),
						version: modConfig.Mod.Version ?? ''
					};
				}
			}
		} finally {
			engineModsDir.closeSync();
		}

		return mods;
	}

	public doesConfigExist(modId: string) {
		const modIniPath = this.getModIniPath(modId);
		const modConfig = ini.fromFileOrDefault(modIniPath);

		return !!modConfig.Mod?.LibraryFileName;
	}

	public getModConfig(modId: string) {
		const modIniPath = this.getModIniPath(modId);
		const modConfig = ini.fromFileOrDefault(modIniPath);

		if (!modConfig.Mod?.LibraryFileName) {
			return null;
		}

		return {
			libraryFileName: modConfig.Mod.LibraryFileName,
			disabled: !!parseInt(modConfig.Mod.Disabled ?? '0', 10),
			loggingEnabled: !!parseInt(modConfig.Mod.LoggingEnabled ?? '0', 10),
			debugLoggingEnabled: !!parseInt(modConfig.Mod.DebugLoggingEnabled ?? '0', 10),
			include: (modConfig.Mod.Include ?? '').split('|'),
			exclude: (modConfig.Mod.Exclude ?? '').split('|'),
			includeCustom: (modConfig.Mod.IncludeCustom ?? '').split('|'),
			excludeCustom: (modConfig.Mod.ExcludeCustom ?? '').split('|'),
			includeExcludeCustomOnly: !!parseInt(modConfig.Mod.IncludeExcludeCustomOnly ?? '0', 10),
			architecture: (modConfig.Mod.Architecture ?? '').split('|'),
			version: modConfig.Mod.Version ?? ''
		};
	}

	private ModSettingsToIniSection(settings: ModSettings) {
		const value: Record<string, string> = {};
		for (const [k, v] of Object.entries(settings)) {
			value[k] = v.toString();
		}

		return value;
	}

	public setModConfig(modId: string, config: Partial<ModConfig>, initialSettings?: ModSettings) {
		const modIniPath = this.getModIniPath(modId);
		const modConfig = ini.fromFileOrDefault(modIniPath);

		modConfig.Mod = modConfig.Mod || {};
		modConfig.Settings = modConfig.Settings || {};

		const configExisted = !!modConfig.Mod.LibraryFileName;

		if (config.libraryFileName !== undefined) {
			modConfig.Mod.LibraryFileName = config.libraryFileName;
		}
		if (config.disabled !== undefined) {
			modConfig.Mod.Disabled = config.disabled ? '1' : '0';
		}
		if (config.loggingEnabled !== undefined) {
			modConfig.Mod.LoggingEnabled = config.loggingEnabled ? '1' : '0';
		}
		if (config.debugLoggingEnabled !== undefined) {
			modConfig.Mod.DebugLoggingEnabled = config.debugLoggingEnabled ? '1' : '0';
		}
		if (config.include !== undefined) {
			modConfig.Mod.Include = config.include.join('|');
		}
		if (config.exclude !== undefined) {
			modConfig.Mod.Exclude = config.exclude.join('|');
		}
		if (config.includeCustom !== undefined) {
			modConfig.Mod.IncludeCustom = config.includeCustom.join('|');
		}
		if (config.excludeCustom !== undefined) {
			modConfig.Mod.ExcludeCustom = config.excludeCustom.join('|');
		}
		if (config.includeExcludeCustomOnly !== undefined) {
			modConfig.Mod.IncludeExcludeCustomOnly = config.includeExcludeCustomOnly ? '1' : '0';
		}
		if (config.architecture !== undefined) {
			modConfig.Mod.Architecture = config.architecture.join('|');
		}
		if (config.version !== undefined) {
			modConfig.Mod.Version = config.version;
		}

		if (initialSettings !== undefined) {
			if (!configExisted) {
				modConfig.Mod.SettingsChangeTime = getSettingsChangeTime().toString();
				modConfig.Settings = this.ModSettingsToIniSection(initialSettings);
			} else {
				const { mergedSettings, existingSettingsChanged } =
					mergeModSettings(modConfig.Settings || {}, initialSettings);
				if (existingSettingsChanged) {
					modConfig.Mod.SettingsChangeTime = getSettingsChangeTime().toString();
					modConfig.Settings = this.ModSettingsToIniSection(mergedSettings);
				}
			}
		}

		fs.mkdirSync(path.dirname(modIniPath), { recursive: true });
		ini.toFile(modIniPath, modConfig);
	}

	public getModSettings(modId: string) {
		let settings: ModSettings = {};

		const modIniPath = this.getModIniPath(modId);
		const modConfig = ini.fromFileOrDefault(modIniPath);

		if (modConfig.Settings) {
			settings = modConfig.Settings;
		}

		return settings;
	}

	public setModSettings(modId: string, settings: ModSettings) {
		const modIniPath = this.getModIniPath(modId);
		const modConfig = ini.fromFileOrDefault(modIniPath);

		modConfig.Settings = this.ModSettingsToIniSection(settings);

		modConfig.Mod = modConfig.Mod || {};
		modConfig.Mod.SettingsChangeTime = getSettingsChangeTime().toString();

		fs.mkdirSync(path.dirname(modIniPath), { recursive: true });
		ini.toFile(modIniPath, modConfig);
	}

	public enableMod(modId: string, enable: boolean) {
		const modIniPath = this.getModIniPath(modId);
		const modConfig = ini.fromFileOrDefault(modIniPath);

		modConfig.Mod = modConfig.Mod || {};
		modConfig.Mod.Disabled = enable ? '0' : '1';

		fs.mkdirSync(path.dirname(modIniPath), { recursive: true });
		ini.toFile(modIniPath, modConfig);
	}

	public enableLogging(modId: string, enable: boolean) {
		const modIniPath = this.getModIniPath(modId);
		const modConfig = ini.fromFileOrDefault(modIniPath);

		modConfig.Mod = modConfig.Mod || {};
		modConfig.Mod.LoggingEnabled = enable ? '1' : '0';

		fs.mkdirSync(path.dirname(modIniPath), { recursive: true });
		ini.toFile(modIniPath, modConfig);
	}

	public deleteMod(modId: string) {
		const modIniPath = this.getModIniPath(modId);
		try {
			fs.unlinkSync(modIniPath);
		} catch (e) {
			// Ignore if file doesn't exist.
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}

		const modWritableIniPath = this.getModWritableIniPath(modId);
		try {
			fs.unlinkSync(modWritableIniPath);
		} catch (e) {
			// Ignore if file doesn't exist.
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}
	}

	public changeModId(modIdFrom: string, modIdTo: string) {
		const modIniPathFrom = this.getModIniPath(modIdFrom);
		const modIniPathTo = this.getModIniPath(modIdTo);
		try {
			fs.renameSync(modIniPathFrom, modIniPathTo);
		} catch (e) {
			// Ignore if file doesn't exist.
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}

		const modWritableIniPathFrom = this.getModWritableIniPath(modIdFrom);
		const modWritableIniPathTo = this.getModWritableIniPath(modIdTo);
		try {
			fs.renameSync(modWritableIniPathFrom, modWritableIniPathTo);
		} catch (e) {
			// Ignore if file doesn't exist.
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}
	}
}

export class ModConfigUtilsNonPortable implements ModConfigUtils {
	private regKey: reg.HKEY;
	private regSubKey: string;

	public constructor(regKey: reg.HKEY, regSubKey: string) {
		this.regKey = regKey;
		this.regSubKey = regSubKey + '\\Engine\\Mods';
	}

	public getConfigOfInstalled() {
		const mods: Record<string, ModConfig> = {};

		const key = reg.openKey(this.regKey, this.regSubKey,
			reg.Access.QUERY_VALUE | reg.Access.ENUMERATE_SUB_KEYS | reg.Access.WOW64_64KEY);
		if (key) {
			try {
				for (const modId of reg.enumKeyNames(key)) {
					const libraryFileName = reg.getValue(key, modId, 'LibraryFileName', reg.GetValueFlags.RT_REG_SZ) as string | null;
					if (!libraryFileName) {
						continue;
					}

					mods[modId] = {
						libraryFileName,
						disabled: !!reg.getValue(key, modId, 'Disabled', reg.GetValueFlags.RT_REG_DWORD),
						loggingEnabled: !!reg.getValue(key, modId, 'LoggingEnabled', reg.GetValueFlags.RT_REG_DWORD),
						debugLoggingEnabled: !!reg.getValue(key, modId, 'DebugLoggingEnabled', reg.GetValueFlags.RT_REG_DWORD),
						include: ((reg.getValue(key, modId, 'Include', reg.GetValueFlags.RT_REG_SZ) ?? '') as string).split('|'),
						exclude: ((reg.getValue(key, modId, 'Exclude', reg.GetValueFlags.RT_REG_SZ) ?? '') as string).split('|'),
						includeCustom: ((reg.getValue(key, modId, 'IncludeCustom', reg.GetValueFlags.RT_REG_SZ) ?? '') as string).split('|'),
						excludeCustom: ((reg.getValue(key, modId, 'ExcludeCustom', reg.GetValueFlags.RT_REG_SZ) ?? '') as string).split('|'),
						includeExcludeCustomOnly: !!reg.getValue(key, modId, 'IncludeExcludeCustomOnly', reg.GetValueFlags.RT_REG_DWORD),
						architecture: ((reg.getValue(key, modId, 'Architecture', reg.GetValueFlags.RT_REG_SZ) ?? '') as string).split('|'),
						version: (reg.getValue(key, modId, 'Version', reg.GetValueFlags.RT_REG_SZ) ?? '') as string
					};
				}
			} finally {
				reg.closeKey(key);
			}
		}

		return mods;
	}

	public doesConfigExist(modId: string) {
		const key = reg.openKey(this.regKey, this.regSubKey + '\\' + modId,
			reg.Access.QUERY_VALUE | reg.Access.WOW64_64KEY);
		if (!key) {
			return false;
		}

		try {
			return !!reg.getValue(key, null, 'LibraryFileName', reg.GetValueFlags.RT_REG_SZ);
		} finally {
			reg.closeKey(key);
		}
	}

	public getModConfig(modId: string) {
		const key = reg.openKey(this.regKey, this.regSubKey + '\\' + modId,
			reg.Access.QUERY_VALUE | reg.Access.WOW64_64KEY);
		if (!key) {
			return null;
		}

		try {
			const libraryFileName = reg.getValue(key, null, 'LibraryFileName', reg.GetValueFlags.RT_REG_SZ) as string | null;
			if (!libraryFileName) {
				return null;
			}

			return {
				libraryFileName,
				disabled: !!reg.getValue(key, null, 'Disabled', reg.GetValueFlags.RT_REG_DWORD),
				loggingEnabled: !!reg.getValue(key, null, 'LoggingEnabled', reg.GetValueFlags.RT_REG_DWORD),
				debugLoggingEnabled: !!reg.getValue(key, null, 'DebugLoggingEnabled', reg.GetValueFlags.RT_REG_DWORD),
				include: ((reg.getValue(key, null, 'Include', reg.GetValueFlags.RT_REG_SZ) ?? '') as string).split('|'),
				exclude: ((reg.getValue(key, null, 'Exclude', reg.GetValueFlags.RT_REG_SZ) ?? '') as string).split('|'),
				includeCustom: ((reg.getValue(key, null, 'IncludeCustom', reg.GetValueFlags.RT_REG_SZ) ?? '') as string).split('|'),
				excludeCustom: ((reg.getValue(key, null, 'ExcludeCustom', reg.GetValueFlags.RT_REG_SZ) ?? '') as string).split('|'),
				includeExcludeCustomOnly: !!reg.getValue(key, null, 'IncludeExcludeCustomOnly', reg.GetValueFlags.RT_REG_DWORD),
				architecture: ((reg.getValue(key, null, 'Architecture', reg.GetValueFlags.RT_REG_SZ) ?? '') as string).split('|'),
				version: (reg.getValue(key, null, 'Version', reg.GetValueFlags.RT_REG_SZ) ?? '') as string
			};
		} finally {
			reg.closeKey(key);
		}
	}

	public setModConfig(modId: string, config: Partial<ModConfig>, initialSettings?: ModSettings) {
		let configExisted = false;

		const key = reg.createKey(this.regKey, this.regSubKey + '\\' + modId,
			reg.Access.QUERY_VALUE | reg.Access.SET_VALUE | reg.Access.WOW64_64KEY);
		try {
			const prevLibraryFileName = reg.getValue(key, null, 'LibraryFileName', reg.GetValueFlags.RT_REG_SZ);
			if (prevLibraryFileName) {
				configExisted = true;
			}

			if (config.libraryFileName !== undefined) {
				reg.setValueSZ(key, 'LibraryFileName', config.libraryFileName);
			}
			if (config.disabled !== undefined) {
				reg.setValueDWORD(key, 'Disabled', config.disabled ? 1 : 0);
			}
			if (config.loggingEnabled !== undefined) {
				reg.setValueDWORD(key, 'LoggingEnabled', config.loggingEnabled ? 1 : 0);
			}
			if (config.debugLoggingEnabled !== undefined) {
				reg.setValueDWORD(key, 'DebugLoggingEnabled', config.debugLoggingEnabled ? 1 : 0);
			}
			if (config.include !== undefined) {
				reg.setValueSZ(key, 'Include', config.include.join('|'));
			}
			if (config.exclude !== undefined) {
				reg.setValueSZ(key, 'Exclude', config.exclude.join('|'));
			}
			if (config.includeCustom !== undefined) {
				reg.setValueSZ(key, 'IncludeCustom', config.includeCustom.join('|'));
			}
			if (config.excludeCustom !== undefined) {
				reg.setValueSZ(key, 'ExcludeCustom', config.excludeCustom.join('|'));
			}
			if (config.includeExcludeCustomOnly !== undefined) {
				reg.setValueDWORD(key, 'IncludeExcludeCustomOnly', config.includeExcludeCustomOnly ? 1 : 0);
			}
			if (config.architecture !== undefined) {
				reg.setValueSZ(key, 'Architecture', config.architecture.join('|'));
			}
			if (config.version !== undefined) {
				reg.setValueSZ(key, 'Version', config.version);
			}
		} finally {
			reg.closeKey(key);
		}

		if (initialSettings !== undefined) {
			if (!configExisted) {
				this.setModSettings(modId, initialSettings);
			} else {
				const { mergedSettings, existingSettingsChanged } =
					mergeModSettings(this.getModSettings(modId), initialSettings);
				if (existingSettingsChanged) {
					this.setModSettings(modId, mergedSettings);
				}
			}
		}
	}

	public getModSettings(modId: string) {
		const settings: ModSettings = {};

		const key = reg.openKey(this.regKey, this.regSubKey + '\\' + modId + '\\Settings',
			reg.Access.QUERY_VALUE | reg.Access.WOW64_64KEY);
		if (key) {
			try {
				for (const valueName of reg.enumValueNames(key)) {
					const value = reg.getValue(key, null, valueName, reg.GetValueFlags.RT_REG_DWORD | reg.GetValueFlags.RT_REG_SZ);
					if (value !== null) {
						if (typeof value === 'number') {
							// Add `| 0` after every math operation to get a
							// 32-bit signed integer result.
							// https://james.darpinian.com/blog/integer-math-in-javascript#tldr
							const valueSigned = value | 0;
							settings[valueName] = valueSigned;
						} else {
							settings[valueName] = value as string;
						}
					}
				}
			} finally {
				reg.closeKey(key);
			}
		}

		return settings;
	}

	public setModSettings(modId: string, settings: ModSettings) {
		const settingsKey = reg.createKey(this.regKey, this.regSubKey + '\\' + modId + '\\Settings',
			reg.Access.QUERY_VALUE | reg.Access.SET_VALUE | reg.Access.DELETE | reg.Access.ENUMERATE_SUB_KEYS | reg.Access.WOW64_64KEY);
		try {
			reg.deleteTree(settingsKey, null);

			for (const [name, value] of Object.entries(settings)) {
				if (typeof value === 'number') {
					// Add [...] `>>> 0` for a 32-bit unsigned integer result.
					// https://james.darpinian.com/blog/integer-math-in-javascript#tldr
					const valueUnsigned = value >>> 0;
					reg.setValueDWORD(settingsKey, name, valueUnsigned);
				} else {
					reg.setValueSZ(settingsKey, name, value);
				}
			}
		} finally {
			reg.closeKey(settingsKey);
		}

		const modKey = reg.createKey(this.regKey, this.regSubKey + '\\' + modId,
			reg.Access.SET_VALUE | reg.Access.WOW64_64KEY);
		try {
			reg.setValueDWORD(modKey, 'SettingsChangeTime', getSettingsChangeTime());
		} finally {
			reg.closeKey(modKey);
		}
	}

	public enableMod(modId: string, enable: boolean) {
		const key = reg.createKey(this.regKey, this.regSubKey + '\\' + modId,
			reg.Access.SET_VALUE | reg.Access.WOW64_64KEY);
		try {
			reg.setValueDWORD(key, 'Disabled', enable ? 0 : 1);
		} finally {
			reg.closeKey(key);
		}
	}

	public enableLogging(modId: string, enable: boolean) {
		const key = reg.createKey(this.regKey, this.regSubKey + '\\' + modId,
			reg.Access.SET_VALUE | reg.Access.WOW64_64KEY);
		try {
			reg.setValueDWORD(key, 'LoggingEnabled', enable ? 1 : 0);
		} finally {
			reg.closeKey(key);
		}
	}

	public deleteMod(modId: string) {
		const key = reg.openKey(this.regKey, this.regSubKey + '\\' + modId,
			reg.Access.QUERY_VALUE | reg.Access.SET_VALUE | reg.Access.DELETE | reg.Access.ENUMERATE_SUB_KEYS | reg.Access.WOW64_64KEY);
		if (key) {
			try {
				if (reg.deleteTree(key, null)) {
					reg.deleteKey(key, '');
				}
			} finally {
				reg.closeKey(key);
			}
		}
	}

	public changeModId(modIdFrom: string, modIdTo: string) {
		const key = reg.openKey(this.regKey, this.regSubKey + '\\' + modIdFrom,
			reg.Access.WRITE | reg.Access.WOW64_64KEY);
		if (key) {
			try {
				reg.renameKey(key, null, modIdTo);
			} finally {
				reg.closeKey(key);
			}
		}
	}
}
