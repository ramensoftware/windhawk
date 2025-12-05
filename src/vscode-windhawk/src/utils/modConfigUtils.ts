import * as fs from 'fs';
import * as reg from 'native-reg';
import * as path from 'path';
import * as ini from '../ini';

type ModSettings = Record<string, string | number>;

type ModSettingsConfig = {
	initialSettings: ModSettings,
	previousInitialSettings?: ModSettings
};

// Field descriptor for automated parsing/serialization
type FieldType = 'string' | 'boolean' | 'string-array';

interface FieldDescriptor {
	name: string;
	storageName: string;
	type: FieldType;
}

const CONFIG_FIELDS = [
	{ name: 'libraryFileName', storageName: 'LibraryFileName', type: 'string' },
	{ name: 'disabled', storageName: 'Disabled', type: 'boolean' },
	{ name: 'loggingEnabled', storageName: 'LoggingEnabled', type: 'boolean' },
	{ name: 'debugLoggingEnabled', storageName: 'DebugLoggingEnabled', type: 'boolean' },
	{ name: 'include', storageName: 'Include', type: 'string-array' },
	{ name: 'exclude', storageName: 'Exclude', type: 'string-array' },
	{ name: 'includeCustom', storageName: 'IncludeCustom', type: 'string-array' },
	{ name: 'excludeCustom', storageName: 'ExcludeCustom', type: 'string-array' },
	{ name: 'includeExcludeCustomOnly', storageName: 'IncludeExcludeCustomOnly', type: 'boolean' },
	{ name: 'patternsMatchCriticalSystemProcesses', storageName: 'PatternsMatchCriticalSystemProcesses', type: 'boolean' },
	{ name: 'architecture', storageName: 'Architecture', type: 'string-array' },
	{ name: 'version', storageName: 'Version', type: 'string' }
] as const satisfies readonly FieldDescriptor[];

// Map field types to TypeScript types
type FieldTypeToTSType<T extends FieldType> =
	T extends 'string' ? string :
	T extends 'boolean' ? boolean :
	T extends 'string-array' ? string[] :
	never;

// Derive ModConfig type from CONFIG_FIELDS
type ModConfig = {
	[K in typeof CONFIG_FIELDS[number] as K['name']]: FieldTypeToTSType<K['type']>
};

// Extract valid storage field names from CONFIG_FIELDS
type StorageFieldName = typeof CONFIG_FIELDS[number]['storageName'];

// Storage abstraction layer
interface ModStorageBackend {
	// Config operations
	readAllConfigFields(modId: string): Partial<Record<StorageFieldName, string | number>> | null;
	writeAllConfigFields(modId: string, fields: Partial<Record<StorageFieldName, string | number>>): void;
	writeConfigField(modId: string, field: StorageFieldName, value: string | number): void;
	configExists(modId: string): boolean;

	// Settings operations
	readAllSettings(modId: string): Record<string, string | number>;
	writeAllSettings(modId: string, settings: Record<string, string | number>): void;

	// Lifecycle operations
	deleteConfig(modId: string): void;
	renameConfig(fromId: string, toId: string): void;

	// Bulk operations
	getConfigOfInstalled(): Record<string, ModConfig>;
}

// Unified codec for ModConfig parsing/serialization
class ModConfigCodec {
	static parse(backend: ModStorageBackend, modId: string): ModConfig | null {
		// Batch read all fields at once for performance
		const rawFields = backend.readAllConfigFields(modId);
		if (!rawFields) {
			return null;
		}

		const libraryFileName = rawFields['LibraryFileName'];
		if (!libraryFileName || typeof libraryFileName !== 'string') {
			return null;
		}

		// Build config object field by field with proper typing
		const config: Partial<ModConfig> = {};

		for (const field of CONFIG_FIELDS) {
			const rawValue = rawFields[field.storageName];

			switch (field.type) {
				case 'string':
					config[field.name] = (rawValue ?? '') as string;
					break;
				case 'boolean':
					config[field.name] = !!rawValue;
					break;
				case 'string-array':
					config[field.name] = splitPipeDelimited((rawValue ?? '') as string);
					break;
			}
		}

		// All fields should be populated at this point
		return config as ModConfig;
	}

	static serialize(backend: ModStorageBackend, modId: string, config: Partial<ModConfig>): void {
		const fieldsToWrite: Partial<Record<StorageFieldName, string | number>> = {};

		for (const field of CONFIG_FIELDS) {
			const value = config[field.name];
			if (value === undefined) {
				continue;
			}

			let storageValue: string | number;

			switch (field.type) {
				case 'string':
					storageValue = value as string;
					break;
				case 'boolean':
					storageValue = (value as boolean) ? 1 : 0;
					break;
				case 'string-array':
					storageValue = (value as string[]).join('|');
					break;
			}

			fieldsToWrite[field.storageName] = storageValue;
		}

		// Batch write all fields at once for performance
		backend.writeAllConfigFields(modId, fieldsToWrite);
	}
}

function getSettingsChangeTime() {
	// Unix timestamp in seconds, limited to a positive signed 32-bit integer.
	return (Date.now() / 1000) & 0x7fffffff;
}

function splitPipeDelimited(value: string): string[] {
	return !value ? [] : value.split('|');
}

function mergeModSettings(existingSettings: ModSettings, newSettings: ModSettings) {
	const getNamePrefix = (name: string) => {
		// Treat each option individually, except for arrays. For arrays, only
		// consider the prefix before the first [index] - if any settings
		// already exist with that prefix, don't add any other settings with the
		// same prefix.
		return name.replace(/\[\d+\].*$/, '[0]');
	};

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

function getModStoragePath(engineModsWritablePath: string, modId: string) {
	return path.join(engineModsWritablePath, 'mod-storage', modId);
}

function deleteModStoragePath(engineModsWritablePath: string, modId: string): void {
	const modStoragePath = getModStoragePath(engineModsWritablePath, modId);
	try {
		fs.rmSync(modStoragePath, { recursive: true, force: true });
	} catch (e) {
		// Ignore errors.
	}
}

// INI-based storage backend (portable mode)
class IniStorageBackend implements ModStorageBackend {
	private engineModsPath: string;
	private engineModsWritablePath: string;

	constructor(appDataPath: string) {
		this.engineModsPath = path.join(appDataPath, 'Engine', 'Mods');
		this.engineModsWritablePath = path.join(appDataPath, 'Engine', 'ModsWritable');
	}

	private getModIniPath(modId: string) {
		return path.join(this.engineModsPath, modId + '.ini');
	}

	private getModWritableIniPath(modId: string) {
		return path.join(this.engineModsWritablePath, modId + '.ini');
	}

	readAllConfigFields(modId: string): Partial<Record<StorageFieldName, string | number>> | null {
		const modIniPath = this.getModIniPath(modId);
		const modConfig = ini.fromFileOrDefault(modIniPath);

		if (!modConfig.Mod) {
			return null;
		}

		const result: Partial<Record<StorageFieldName, string | number>> = {};
		for (const field of CONFIG_FIELDS) {
			const value = modConfig.Mod[field.storageName];
			if (value !== undefined) {
				// Convert string representations to appropriate types
				if (field.type === 'boolean') {
					result[field.storageName] = parseInt(value, 10);
				} else {
					result[field.storageName] = value;
				}
			}
		}

		return result;
	}

	writeAllConfigFields(modId: string, fields: Partial<Record<StorageFieldName, string | number>>): void {
		const modIniPath = this.getModIniPath(modId);
		const modConfig = ini.fromFileOrDefault(modIniPath);

		modConfig.Mod = modConfig.Mod || {};
		for (const [field, value] of Object.entries(fields)) {
			modConfig.Mod[field] = value.toString();
		}

		fs.mkdirSync(path.dirname(modIniPath), { recursive: true });
		ini.toFile(modIniPath, modConfig);
	}

	writeConfigField(modId: string, field: StorageFieldName, value: string | number): void {
		this.writeAllConfigFields(modId, { [field]: value });
	}

	configExists(modId: string): boolean {
		const modIniPath = this.getModIniPath(modId);
		const modConfig = ini.fromFileOrDefault(modIniPath);
		return !!modConfig.Mod?.LibraryFileName;
	}

	readAllSettings(modId: string): Record<string, string | number> {
		const modIniPath = this.getModIniPath(modId);
		const modConfig = ini.fromFileOrDefault(modIniPath);
		return modConfig.Settings || {};
	}

	writeAllSettings(modId: string, settings: Record<string, string | number>): void {
		const modIniPath = this.getModIniPath(modId);
		const modConfig = ini.fromFileOrDefault(modIniPath);

		const settingsSection: Record<string, string> = {};
		for (const [k, v] of Object.entries(settings)) {
			settingsSection[k] = v.toString();
		}

		modConfig.Settings = settingsSection;
		modConfig.Mod = modConfig.Mod || {};
		modConfig.Mod.SettingsChangeTime = getSettingsChangeTime().toString();

		fs.mkdirSync(path.dirname(modIniPath), { recursive: true });
		ini.toFile(modIniPath, modConfig);
	}

	deleteConfig(modId: string): void {
		const modIniPath = this.getModIniPath(modId);
		try {
			fs.unlinkSync(modIniPath);
		} catch (e: any) {
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}

		const modWritableIniPath = this.getModWritableIniPath(modId);
		try {
			fs.unlinkSync(modWritableIniPath);
		} catch (e: any) {
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}

		deleteModStoragePath(this.engineModsWritablePath, modId);
	}

	renameConfig(fromId: string, toId: string): void {
		const modIniPathFrom = this.getModIniPath(fromId);
		const modIniPathTo = this.getModIniPath(toId);
		try {
			fs.renameSync(modIniPathFrom, modIniPathTo);
		} catch (e: any) {
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}

		const modWritableIniPathFrom = this.getModWritableIniPath(fromId);
		const modWritableIniPathTo = this.getModWritableIniPath(toId);
		try {
			fs.renameSync(modWritableIniPathFrom, modWritableIniPathTo);
		} catch (e: any) {
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}
	}

	getConfigOfInstalled(): Record<string, ModConfig> {
		const mods: Record<string, ModConfig> = {};

		let engineModsDir: fs.Dir;
		try {
			engineModsDir = fs.opendirSync(this.engineModsPath);
		} catch (e: any) {
			if (e.code !== 'ENOENT') {
				throw e;
			}
			return mods;
		}

		try {
			let engineModsDirEntry: fs.Dirent | null;
			while ((engineModsDirEntry = engineModsDir.readSync()) !== null) {
				if (engineModsDirEntry.isFile() && engineModsDirEntry.name.endsWith('.ini')) {
					const modId = engineModsDirEntry.name.slice(0, -'.ini'.length);
					const config = ModConfigCodec.parse(this, modId);
					if (config) {
						mods[modId] = config;
					}
				}
			}
		} finally {
			engineModsDir.closeSync();
		}

		return mods;
	}
}

// Registry-based storage backend (non-portable mode)
class RegistryStorageBackend implements ModStorageBackend {
	private regKey: reg.HKEY;
	private regSubKey: string;
	private regSubKeyModWritable: string;
	private engineModsWritablePath: string;

	constructor(regKey: reg.HKEY, regSubKey: string, appDataPath: string) {
		this.regKey = regKey;
		this.regSubKey = regSubKey + '\\Engine\\Mods';
		this.regSubKeyModWritable = regSubKey + '\\Engine\\ModsWritable';
		this.engineModsWritablePath = path.join(appDataPath, 'Engine', 'ModsWritable');
	}

	readAllConfigFields(modId: string): Partial<Record<StorageFieldName, string | number>> | null {
		const key = reg.openKey(this.regKey, this.regSubKey + '\\' + modId,
			reg.Access.QUERY_VALUE | reg.Access.WOW64_64KEY);
		if (!key) {
			return null;
		}

		try {
			const result: Partial<Record<StorageFieldName, string | number>> = {};
			for (const field of CONFIG_FIELDS) {
				const isDword = field.type === 'boolean';
				let value: string | number | null;

				if (isDword) {
					value = reg.getValue(key, null, field.storageName, reg.GetValueFlags.RT_REG_DWORD) as number | null;
				} else {
					value = reg.getValue(key, null, field.storageName, reg.GetValueFlags.RT_REG_SZ) as string | null;
				}

				if (value !== null) {
					result[field.storageName] = value;
				}
			}

			return result;
		} finally {
			reg.closeKey(key);
		}
	}

	writeAllConfigFields(modId: string, fields: Partial<Record<StorageFieldName, string | number>>): void {
		const key = reg.createKey(this.regKey, this.regSubKey + '\\' + modId,
			reg.Access.SET_VALUE | reg.Access.WOW64_64KEY);
		try {
			for (const [field, value] of Object.entries(fields)) {
				if (typeof value === 'number') {
					reg.setValueDWORD(key, field, value);
				} else {
					reg.setValueSZ(key, field, value);
				}
			}
		} finally {
			reg.closeKey(key);
		}
	}

	writeConfigField(modId: string, field: StorageFieldName, value: string | number): void {
		this.writeAllConfigFields(modId, { [field]: value });
	}

	configExists(modId: string): boolean {
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

	readAllSettings(modId: string): Record<string, string | number> {
		const settings: Record<string, string | number> = {};

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

	writeAllSettings(modId: string, settings: Record<string, string | number>): void {
		const settingsKey = reg.createKey(this.regKey, this.regSubKey + '\\' + modId + '\\Settings',
			reg.Access.QUERY_VALUE | reg.Access.SET_VALUE | reg.Access.DELETE | reg.Access.ENUMERATE_SUB_KEYS | reg.Access.WOW64_64KEY);
		try {
			reg.deleteTree(settingsKey, null);

			for (const [name, value] of Object.entries(settings)) {
				if (typeof value === 'number') {
					// Add [...] `>>> 0` for a 32-bit unsigned integer result.
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

	deleteConfig(modId: string): void {
		for (const subKey of [this.regSubKey, this.regSubKeyModWritable]) {
			const key = reg.openKey(this.regKey, subKey + '\\' + modId,
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

		deleteModStoragePath(this.engineModsWritablePath, modId);
	}

	renameConfig(fromId: string, toId: string): void {
		for (const subKey of [this.regSubKey, this.regSubKeyModWritable]) {
			const key = reg.openKey(this.regKey, subKey + '\\' + fromId,
				reg.Access.WRITE | reg.Access.WOW64_64KEY);
			if (key) {
				try {
					reg.renameKey(key, null, toId);
				} finally {
					reg.closeKey(key);
				}
			}
		}
	}

	getConfigOfInstalled(): Record<string, ModConfig> {
		const mods: Record<string, ModConfig> = {};

		const key = reg.openKey(this.regKey, this.regSubKey,
			reg.Access.QUERY_VALUE | reg.Access.ENUMERATE_SUB_KEYS | reg.Access.WOW64_64KEY);
		if (key) {
			try {
				for (const modId of reg.enumKeyNames(key)) {
					const config = ModConfigCodec.parse(this, modId);
					if (config) {
						mods[modId] = config;
					}
				}
			} finally {
				reg.closeKey(key);
			}
		}

		return mods;
	}
}

export interface ModConfigUtils {
	getConfigOfInstalled(): Record<string, ModConfig>;
	doesConfigExist(modId: string): boolean;
	getModConfig(modId: string): ModConfig | null;
	setModConfig(modId: string, config: Partial<ModConfig>, settingsConfig?: ModSettingsConfig): void;
	getModSettings(modId: string): ModSettings;
	setModSettings(modId: string, settings: ModSettings): void;
	enableMod(modId: string, enable: boolean): void;
	enableLogging(modId: string, enable: boolean): void;
	deleteMod(modId: string): void;
	changeModId(modIdFrom: string, modIdTo: string): void;
}

// Base implementation using storage backend pattern
class ModConfigUtilsBase implements ModConfigUtils {
	protected backend: ModStorageBackend;

	protected constructor(backend: ModStorageBackend) {
		this.backend = backend;
	}

	public getConfigOfInstalled() {
		return this.backend.getConfigOfInstalled();
	}

	public doesConfigExist(modId: string) {
		return this.backend.configExists(modId);
	}

	public getModConfig(modId: string) {
		return ModConfigCodec.parse(this.backend, modId);
	}

	public setModConfig(modId: string, config: Partial<ModConfig>, settingsConfig?: ModSettingsConfig) {
		const configExisted = this.backend.configExists(modId);

		ModConfigCodec.serialize(this.backend, modId, config);

		if (settingsConfig) {
			if (!settingsConfig.previousInitialSettings && !configExisted) {
				this.backend.writeAllSettings(modId, settingsConfig.initialSettings);
			} else {
				const { mergedSettings, existingSettingsChanged } =
					mergeModSettings({
						...(settingsConfig.previousInitialSettings || {}),
						...this.backend.readAllSettings(modId)
					}, settingsConfig.initialSettings);
				if (existingSettingsChanged) {
					this.backend.writeAllSettings(modId, mergedSettings);
				}
			}
		}
	}

	public getModSettings(modId: string) {
		return this.backend.readAllSettings(modId);
	}

	public setModSettings(modId: string, settings: ModSettings) {
		this.backend.writeAllSettings(modId, settings);
	}

	public enableMod(modId: string, enable: boolean) {
		this.backend.writeConfigField(modId, 'Disabled', enable ? 0 : 1);
	}

	public enableLogging(modId: string, enable: boolean) {
		this.backend.writeConfigField(modId, 'LoggingEnabled', enable ? 1 : 0);
	}

	public deleteMod(modId: string) {
		this.backend.deleteConfig(modId);
	}

	public changeModId(modIdFrom: string, modIdTo: string) {
		this.backend.renameConfig(modIdFrom, modIdTo);
	}
}

export class ModConfigUtilsPortable extends ModConfigUtilsBase {
	public constructor(appDataPath: string) {
		super(new IniStorageBackend(appDataPath));
	}
}

export class ModConfigUtilsNonPortable extends ModConfigUtilsBase {
	public constructor(regKey: reg.HKEY, regSubKey: string, appDataPath: string) {
		super(new RegistryStorageBackend(regKey, regSubKey, appDataPath));
	}
}
