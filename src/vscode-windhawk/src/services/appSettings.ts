import * as child_process from 'child_process';
import * as reg from 'native-reg';
import * as path from 'path';
import { debugInstallerRegKey, debugSchtasksPath } from '../storage/debugOverrides';
import * as ini from '../storage/ini';
import { Logger } from './logger';
import { AppSettings } from './types';

// ============================================================================
// Field Descriptors
// ============================================================================
//
// AppSettings (the public type, imported from ./types) is the source of truth
// for the shape. APP_SETTINGS_FIELDS below describes each field's on-disk
// storage name, type tag, and default. The `as const satisfies ...` clause
// forces:
//   (1) every AppSettings key to have a descriptor entry at the right location
//       (missing entries are a compile error),
//   (2) each entry's type tag to match the key's declared TypeScript type
//       (wrong tag is a compile error).
// So drift between the public type and this descriptor table is impossible.

type FieldValue = string | number | boolean | string[];

type FieldTypeTag<T> =
	T extends string ? 'string' :
	T extends boolean ? 'boolean' :
	T extends number ? 'number' :
	T extends string[] ? 'string-array' :
	never;

type FieldDescriptor<T> = {
	storageName: string;
	type: FieldTypeTag<T>;
	defaultValue?: T;
};

// Internal widened shape used by the codec: disableRunUIScheduledTask is
// a plain boolean (the descriptor declares it as `boolean`). The portable
// subclass substitutes `null` in its getAppSettings override, widening to
// match the public AppSettings type from types.ts.
type DerivedAppSettings = Omit<AppSettings, 'disableRunUIScheduledTask'> & {
	disableRunUIScheduledTask: boolean;
};

type AppLevelFields = Omit<DerivedAppSettings, 'engine'>;
type EngineLevelFields = DerivedAppSettings['engine'];

const APP_SETTINGS_FIELDS = {
	app: {
		language: { storageName: 'Language', type: 'string', defaultValue: 'en' },
		disableUpdateCheck: { storageName: 'DisableUpdateCheck', type: 'boolean' },
		disableRunUIScheduledTask: { storageName: 'DisableRunUIScheduledTask', type: 'boolean' },
		devModeOptOut: { storageName: 'DevModeOptOut', type: 'boolean' },
		hideTrayIcon: { storageName: 'HideTrayIcon', type: 'boolean' },
		alwaysCompileModsLocally: { storageName: 'AlwaysCompileModsLocally', type: 'boolean' },
		dontAutoShowToolkit: { storageName: 'DontAutoShowToolkit', type: 'boolean' },
		modTasksDialogDelay: { storageName: 'ModTasksDialogDelay', type: 'number', defaultValue: 2000 },
		safeMode: { storageName: 'SafeMode', type: 'boolean' },
		loggingVerbosity: { storageName: 'LoggingVerbosity', type: 'number' },
	},
	engine: {
		loggingVerbosity: { storageName: 'LoggingVerbosity', type: 'number' },
		include: { storageName: 'Include', type: 'string-array' },
		exclude: { storageName: 'Exclude', type: 'string-array' },
		injectIntoCriticalProcesses: { storageName: 'InjectIntoCriticalProcesses', type: 'boolean' },
		injectIntoIncompatiblePrograms: { storageName: 'InjectIntoIncompatiblePrograms', type: 'boolean' },
		injectIntoGames: { storageName: 'InjectIntoGames', type: 'boolean' },
	},
} as const satisfies {
	app: { [K in keyof AppLevelFields]: FieldDescriptor<AppLevelFields[K]> };
	engine: { [K in keyof EngineLevelFields]: FieldDescriptor<EngineLevelFields[K]> };
};

type StorageLocation = keyof typeof APP_SETTINGS_FIELDS;

// Distributive entry shape: name, storageName, type live on the same object
// so a switch on `type` narrows `name` too.
type AppFieldEntry = {
	[K in keyof typeof APP_SETTINGS_FIELDS.app]: {
		name: K;
		storageName: typeof APP_SETTINGS_FIELDS.app[K]['storageName'];
		type: typeof APP_SETTINGS_FIELDS.app[K]['type'];
		defaultValue?: FieldValue;
	};
}[keyof typeof APP_SETTINGS_FIELDS.app];

type EngineFieldEntry = {
	[K in keyof typeof APP_SETTINGS_FIELDS.engine]: {
		name: K;
		storageName: typeof APP_SETTINGS_FIELDS.engine[K]['storageName'];
		type: typeof APP_SETTINGS_FIELDS.engine[K]['type'];
		defaultValue?: FieldValue;
	};
}[keyof typeof APP_SETTINGS_FIELDS.engine];

function appFieldEntries(): AppFieldEntry[] {
	return Object.entries(APP_SETTINGS_FIELDS.app).map(
		([name, desc]) => ({ name, ...desc })
	) as AppFieldEntry[];
}

function engineFieldEntries(): EngineFieldEntry[] {
	return Object.entries(APP_SETTINGS_FIELDS.engine).map(
		([name, desc]) => ({ name, ...desc })
	) as EngineFieldEntry[];
}

// Union of every storage name across both locations. Used as the record key
// type the backends read from / write to.
type StorageFieldName =
	| typeof APP_SETTINGS_FIELDS.app[keyof typeof APP_SETTINGS_FIELDS.app]['storageName']
	| typeof APP_SETTINGS_FIELDS.engine[keyof typeof APP_SETTINGS_FIELDS.engine]['storageName'];

// Widened shape of any single descriptor entry (any location, any type). Used
// where the specific-narrow type isn't needed.
type AnyFieldEntry = AppFieldEntry | EngineFieldEntry;

function fieldEntriesAt(location: StorageLocation): AnyFieldEntry[] {
	return location === 'app' ? appFieldEntries() : engineFieldEntries();
}

export interface AppSettingsService {
	getAppSettings(): AppSettings;
	updateAppSettings(appSettings: Partial<AppSettings>): void;
	shouldRestartApp(appSettings: Partial<AppSettings>): boolean;
	shouldNotifyTrayProgram(appSettings: Partial<AppSettings>): boolean;
}

// ============================================================================
// Storage Backend Abstraction
// ============================================================================

interface AppSettingsBackend {
	readAllFields(location: StorageLocation): Partial<Record<StorageFieldName, string | number>> | null;
	writeAllFields(location: StorageLocation, fields: Partial<Record<StorageFieldName, string | number>>): void;
}

// ============================================================================
// Unified Codec - Converts between AppSettings and storage format
// ============================================================================

function splitPipeDelimited(value: string): string[] {
	return !value ? [] : value.split('|');
}

type BasicFieldShape = {
	type: 'string' | 'boolean' | 'number' | 'string-array';
	defaultValue?: FieldValue;
};

function parseRawValue(field: BasicFieldShape, rawValue: string | number | undefined): FieldValue {
	if (rawValue !== undefined) {
		if (field.type === 'string') {
			return typeof rawValue === 'string' ? rawValue : String(rawValue);
		}
		if (field.type === 'boolean') {
			return !!rawValue;
		}
		if (field.type === 'number') {
			return typeof rawValue === 'number' ? rawValue : 0;
		}
		return splitPipeDelimited(typeof rawValue === 'string' ? rawValue : String(rawValue));
	}
	if (field.defaultValue !== undefined) {
		return field.defaultValue;
	}
	return field.type === 'string' ? '' :
		field.type === 'boolean' ? false :
		field.type === 'number' ? 0 :
		[];
}

function toStorageValue(field: BasicFieldShape, value: FieldValue): string | number | undefined {
	if (field.type === 'boolean') {
		return value ? 1 : 0;
	}
	if (field.type === 'number' && typeof value === 'number') {
		return value;
	}
	if (field.type === 'string' && typeof value === 'string') {
		return value;
	}
	if (field.type === 'string-array' && Array.isArray(value)) {
		return value.join('|');
	}
	return undefined;
}

class AppSettingsCodec {
	static parse(backend: AppSettingsBackend): DerivedAppSettings {
		const appRaw = backend.readAllFields('app') || {};
		const engineRaw = backend.readAllFields('engine') || {};

		const appFields: Record<string, FieldValue> = {};
		for (const field of appFieldEntries()) {
			appFields[field.name] = parseRawValue(field, appRaw[field.storageName]);
		}

		const engineFields: Record<string, FieldValue> = {};
		for (const field of engineFieldEntries()) {
			engineFields[field.name] = parseRawValue(field, engineRaw[field.storageName]);
		}

		return { ...appFields, engine: engineFields } as DerivedAppSettings;
	}

	static serialize(backend: AppSettingsBackend, appSettings: Partial<DerivedAppSettings>): void {
		const topLevel = appSettings as Partial<Record<string, FieldValue>>;
		const engineLevel = (appSettings.engine ?? {}) as Partial<Record<string, FieldValue>>;

		const appFieldsToWrite = collectForWrite(appFieldEntries(), topLevel);
		const engineFieldsToWrite = collectForWrite(engineFieldEntries(), engineLevel);

		if (Object.keys(appFieldsToWrite).length > 0) {
			backend.writeAllFields('app', appFieldsToWrite);
		}
		if (Object.keys(engineFieldsToWrite).length > 0) {
			backend.writeAllFields('engine', engineFieldsToWrite);
		}
	}
}

function collectForWrite(
	entries: AnyFieldEntry[],
	values: Partial<Record<string, FieldValue>>,
): Partial<Record<StorageFieldName, string | number>> {
	const result: Partial<Record<StorageFieldName, string | number>> = {};
	for (const field of entries) {
		const value = values[field.name];
		if (value === undefined) {
			continue;
		}
		const storageValue = toStorageValue(field, value);
		if (storageValue === undefined) {
			continue;
		}
		result[field.storageName] = storageValue;
	}
	return result;
}

// ============================================================================
// INI Storage Backend (Portable mode)
// ============================================================================

class IniAppSettingsBackend implements AppSettingsBackend {
	private settingsIniPath: string;
	private engineSettingsIniPath: string;

	constructor(appDataPath: string) {
		this.settingsIniPath = path.join(appDataPath, 'settings.ini');
		this.engineSettingsIniPath = path.join(appDataPath, 'engine', 'settings.ini');
	}

	readAllFields(location: StorageLocation): Partial<Record<StorageFieldName, string | number>> | null {
		const iniPath = location === 'app' ? this.settingsIniPath : this.engineSettingsIniPath;
		const iniFileParsed = ini.fromFileOrDefault(iniPath);
		const settings = iniFileParsed.Settings;
		if (!settings) {
			return {};
		}

		const result: Partial<Record<StorageFieldName, string | number>> = {};

		for (const field of fieldEntriesAt(location)) {
			const value = settings[field.storageName];
			if (value === undefined) {
				continue;
			}

			if (field.type === 'string' || field.type === 'string-array') {
				result[field.storageName] = value;
			} else {
				// number, boolean - all stored as numbers in INI
				const numValue = parseInt(value, 10);
				result[field.storageName] = isNaN(numValue) ? 0 : numValue;
			}
		}

		return result;
	}

	writeAllFields(location: StorageLocation, fields: Partial<Record<StorageFieldName, string | number>>): void {
		const iniPath = location === 'app' ? this.settingsIniPath : this.engineSettingsIniPath;
		const iniFileParsed = ini.fromFileOrDefault(iniPath);
		iniFileParsed.Settings = iniFileParsed.Settings || {};

		for (const [key, value] of Object.entries(fields)) {
			iniFileParsed.Settings[key] = typeof value === 'number' ? value.toString() : value;
		}

		ini.toFile(iniPath, iniFileParsed);
	}
}

// ============================================================================
// Registry Storage Backend (Non-portable mode)
// ============================================================================

class RegistryAppSettingsBackend implements AppSettingsBackend {
	private regKey: reg.HKEY;
	private regSubKey: string;
	private engineRegSubKey: string;

	constructor(regKey: reg.HKEY, regSubKey: string) {
		this.regKey = regKey;
		this.regSubKey = regSubKey + '\\Settings';
		this.engineRegSubKey = regSubKey + '\\Engine\\Settings';
	}

	readAllFields(location: StorageLocation): Partial<Record<StorageFieldName, string | number>> | null {
		const subKey = location === 'app' ? this.regSubKey : this.engineRegSubKey;
		const key = reg.createKey(this.regKey, subKey, reg.Access.QUERY_VALUE | reg.Access.WOW64_64KEY);
		try {
			const result: Partial<Record<StorageFieldName, string | number>> = {};

			for (const field of fieldEntriesAt(location)) {
				if (field.type === 'string' || field.type === 'string-array') {
					const value = reg.getValue(key, null, field.storageName, reg.GetValueFlags.RT_REG_SZ);
					if (value !== null && typeof value === 'string') {
						result[field.storageName] = value;
					}
				} else {
					// number, boolean - all stored as DWORD
					const value = reg.getValue(key, null, field.storageName, reg.GetValueFlags.RT_REG_DWORD);
					if (value !== null && typeof value === 'number') {
						result[field.storageName] = value;
					}
				}
			}

			return result;
		} finally {
			reg.closeKey(key);
		}
	}

	writeAllFields(location: StorageLocation, fields: Partial<Record<StorageFieldName, string | number>>): void {
		const subKey = location === 'app' ? this.regSubKey : this.engineRegSubKey;
		const key = reg.createKey(this.regKey, subKey, reg.Access.SET_VALUE | reg.Access.WOW64_64KEY);
		try {
			for (const [fieldName, value] of Object.entries(fields)) {
				if (typeof value === 'number') {
					reg.setValueDWORD(key, fieldName, value);
				} else {
					reg.setValueSZ(key, fieldName, value);
				}
			}
		} finally {
			reg.closeKey(key);
		}
	}
}

// ============================================================================
// Base Implementation - Common logic for both modes
// ============================================================================

class AppSettingsBase implements AppSettingsService {
	protected backend: AppSettingsBackend;

	protected constructor(backend: AppSettingsBackend) {
		this.backend = backend;
	}

	public getAppSettings(): AppSettings {
		return AppSettingsCodec.parse(this.backend);
	}

	public updateAppSettings(appSettings: Partial<DerivedAppSettings>): void {
		AppSettingsCodec.serialize(this.backend, appSettings);
	}

	public shouldRestartApp(appSettings: Partial<AppSettings>): boolean {
		return appSettings.safeMode !== undefined ||
			appSettings.loggingVerbosity !== undefined ||
			(appSettings.engine !== undefined && Object.keys(appSettings.engine).length > 0);
	}

	public shouldNotifyTrayProgram(appSettings: Partial<AppSettings>): boolean {
		return appSettings.language !== undefined ||
			appSettings.disableUpdateCheck !== undefined ||
			appSettings.hideTrayIcon !== undefined ||
			appSettings.dontAutoShowToolkit !== undefined ||
			appSettings.modTasksDialogDelay !== undefined;
	}
}

// ============================================================================
// Portable Implementation
// ============================================================================

export class AppSettingsPortable extends AppSettingsBase {
	public constructor(appDataPath: string) {
		super(new IniAppSettingsBackend(appDataPath));
	}

	public getAppSettings(): AppSettings {
		return { ...super.getAppSettings(), disableRunUIScheduledTask: null };
	}

	public updateAppSettings(appSettings: Partial<AppSettings>): void {
		// Portable mode has no scheduled task. A present boolean is rejected; a
		// present null (which getAppSettings reports here) is tolerated as a
		// no-op, the same as the field being absent, so a round-tripped settings
		// object is not rejected. The field is stripped before writing.
		if (appSettings.disableRunUIScheduledTask != null) {
			throw new Error('Cannot set disableRunUIScheduledTask in portable mode');
		}
		const { disableRunUIScheduledTask: _, ...rest } = appSettings;
		super.updateAppSettings(rest);
	}
}

// ============================================================================
// Non-Portable Implementation
// ============================================================================

export class AppSettingsNonPortable extends AppSettingsBase {
	private logger: Logger;

	public constructor(regKey: reg.HKEY, regSubKey: string, logger: Logger) {
		super(new RegistryAppSettingsBackend(regKey, regSubKey));
		this.logger = logger;
	}

	public updateAppSettings(appSettings: Partial<AppSettings>): void {
		// Handle special side effects for non-portable mode
		if (appSettings.language !== undefined) {
			try {
				this.setInstallerLanguage(appSettings.language);
			} catch (e) {
				console.warn('Failed to set installer language', e);
			}
		}
		if (appSettings.disableUpdateCheck !== undefined) {
			this.enableScheduledTask('WindhawkUpdateTask', !appSettings.disableUpdateCheck);
		}
		// A present boolean toggles the task (enable iff not disabled); a
		// present null - or the field being absent - is treated as "not
		// touching it": no toggle, no write.
		if (appSettings.disableRunUIScheduledTask != null) {
			this.enableScheduledTask('WindhawkRunUITask', !appSettings.disableRunUIScheduledTask);
		}

		// Narrow to DerivedAppSettings: disableRunUIScheduledTask is always boolean in non-portable mode
		const { disableRunUIScheduledTask, ...rest } = appSettings;
		super.updateAppSettings(disableRunUIScheduledTask != null
			? { ...rest, disableRunUIScheduledTask }
			: rest
		);
	}

	private setInstallerLanguage(language: string) {
		// https://github.com/sindresorhus/lcid/blob/958d38ff2b812d6854cbba2cae611e86a1d5ddf3/lcid.json
		const lcidMap = {
			"4": "zh_CHS",
			"1025": "ar_SA",
			"1026": "bg_BG",
			"1027": "ca_ES",
			"1028": "zh_TW",
			"1029": "cs_CZ",
			"1030": "da_DK",
			"1031": "de_DE",
			"1032": "el_GR",
			"1033": "en_US",
			"1034": "es_ES",
			"1035": "fi_FI",
			"1036": "fr_FR",
			"1037": "he_IL",
			"1038": "hu_HU",
			"1039": "is_IS",
			"1040": "it_IT",
			"1041": "ja_JP",
			"1042": "ko_KR",
			"1043": "nl_NL",
			"1044": "nb_NO",
			"1045": "pl_PL",
			"1046": "pt_BR",
			"1047": "rm_CH",
			"1048": "ro_RO",
			"1049": "ru_RU",
			"1050": "hr_HR",
			"1051": "sk_SK",
			"1052": "sq_AL",
			"1053": "sv_SE",
			"1054": "th_TH",
			"1055": "tr_TR",
			"1056": "ur_PK",
			"1057": "id_ID",
			"1058": "uk_UA",
			"1059": "be_BY",
			"1060": "sl_SI",
			"1061": "et_EE",
			"1062": "lv_LV",
			"1063": "lt_LT",
			"1064": "tg_TJ",
			"1065": "fa_IR",
			"1066": "vi_VN",
			"1067": "hy_AM",
			"1069": "eu_ES",
			"1070": "wen_DE",
			"1071": "mk_MK",
			"1072": "st_ZA",
			"1073": "ts_ZA",
			"1074": "tn_ZA",
			"1075": "ven_ZA",
			"1076": "xh_ZA",
			"1077": "zu_ZA",
			"1078": "af_ZA",
			"1079": "ka_GE",
			"1080": "fo_FO",
			"1081": "hi_IN",
			"1082": "mt_MT",
			"1083": "se_NO",
			"1084": "gd_GB",
			"1085": "yi",
			"1086": "ms_MY",
			"1087": "kk_KZ",
			"1088": "ky_KG",
			"1089": "sw_KE",
			"1090": "tk_TM",
			"1092": "tt_RU",
			"1093": "bn_IN",
			"1094": "pa_IN",
			"1095": "gu_IN",
			"1096": "or_IN",
			"1097": "ta_IN",
			"1098": "te_IN",
			"1099": "kn_IN",
			"1100": "ml_IN",
			"1101": "as_IN",
			"1102": "mr_IN",
			"1103": "sa_IN",
			"1104": "mn_MN",
			"1105": "bo_CN",
			"1106": "cy_GB",
			"1107": "kh_KH",
			"1108": "lo_LA",
			"1109": "my_MM",
			"1110": "gl_ES",
			"1111": "kok_IN",
			"1113": "sd_IN",
			"1114": "syr_SY",
			"1115": "si_LK",
			"1116": "chr_US",
			"1118": "am_ET",
			"1119": "tmz",
			"1121": "ne_NP",
			"1122": "fy_NL",
			"1123": "ps_AF",
			"1124": "fil_PH",
			"1125": "div_MV",
			"1126": "bin_NG",
			"1127": "fuv_NG",
			"1128": "ha_NG",
			"1129": "ibb_NG",
			"1130": "yo_NG",
			"1131": "quz_BO",
			"1132": "ns_ZA",
			"1133": "ba_RU",
			"1134": "lb_LU",
			"1135": "kl_GL",
			"1144": "ii_CN",
			"1146": "arn_CL",
			"1148": "moh_CA",
			"1150": "br_FR",
			"1152": "ug_CN",
			"1153": "mi_NZ",
			"1154": "oc_FR",
			"1155": "co_FR",
			"1156": "gsw_FR",
			"1157": "sah_RU",
			"1158": "qut_GT",
			"1159": "rw_RW",
			"1160": "wo_SN",
			"1164": "gbz_AF",
			"2049": "ar_IQ",
			"2052": "zh_CN",
			"2055": "de_CH",
			"2057": "en_GB",
			"2058": "es_MX",
			"2060": "fr_BE",
			"2064": "it_CH",
			"2067": "nl_BE",
			"2068": "nn_NO",
			"2070": "pt_PT",
			"2072": "ro_MD",
			"2073": "ru_MD",
			"2077": "sv_FI",
			"2080": "ur_IN",
			"2092": "az_AZ",
			"2094": "dsb_DE",
			"2107": "se_SE",
			"2108": "ga_IE",
			"2110": "ms_BN",
			"2115": "uz_UZ",
			"2128": "mn_CN",
			"2129": "bo_BT",
			"2141": "iu_CA",
			"2143": "tmz_DZ",
			"2145": "ne_IN",
			"2155": "quz_EC",
			"2163": "ti_ET",
			"3073": "ar_EG",
			"3076": "zh_HK",
			"3079": "de_AT",
			"3081": "en_AU",
			"3082": "es_ES",
			"3084": "fr_CA",
			"3098": "sr_SP",
			"3131": "se_FI",
			"3179": "quz_PE",
			"4097": "ar_LY",
			"4100": "zh_SG",
			"4103": "de_LU",
			"4105": "en_CA",
			"4106": "es_GT",
			"4108": "fr_CH",
			"4122": "hr_BA",
			"4155": "smj_NO",
			"5121": "ar_DZ",
			"5124": "zh_MO",
			"5127": "de_LI",
			"5129": "en_NZ",
			"5130": "es_CR",
			"5132": "fr_LU",
			"5179": "smj_SE",
			"6145": "ar_MA",
			"6153": "en_IE",
			"6154": "es_PA",
			"6156": "fr_MC",
			"6203": "sma_NO",
			"7169": "ar_TN",
			"7177": "en_ZA",
			"7178": "es_DO",
			"7180": "fr_029",
			"7194": "sr_BA",
			"7227": "sma_SE",
			"8193": "ar_OM",
			"8201": "en_JA",
			"8202": "es_VE",
			"8204": "fr_RE",
			"8218": "bs_BA",
			"8251": "sms_FI",
			"9217": "ar_YE",
			"9225": "en_CB",
			"9226": "es_CO",
			"9228": "fr_CG",
			"9275": "smn_FI",
			"10241": "ar_SY",
			"10249": "en_BZ",
			"10250": "es_PE",
			"10252": "fr_SN",
			"11265": "ar_JO",
			"11273": "en_TT",
			"11274": "es_AR",
			"11276": "fr_CM",
			"12289": "ar_LB",
			"12297": "en_ZW",
			"12298": "es_EC",
			"12300": "fr_CI",
			"13313": "ar_KW",
			"13321": "en_PH",
			"13322": "es_CL",
			"13324": "fr_ML",
			"14337": "ar_AE",
			"14345": "en_ID",
			"14346": "es_UR",
			"14348": "fr_MA",
			"15361": "ar_BH",
			"15369": "en_HK",
			"15370": "es_PY",
			"15372": "fr_HT",
			"16385": "ar_QA",
			"16393": "en_IN",
			"16394": "es_BO",
			"17417": "en_MY",
			"17418": "es_SV",
			"18441": "en_SG",
			"18442": "es_HN",
			"19466": "es_NI",
			"20490": "es_PR",
			"21514": "es_US",
			"31748": "zh_CHT"
		};
		const languageParts = language.split('-');
		let languageLcid: number | undefined;
		for (const [lcid, iterLanguage] of Object.entries(lcidMap)) {
			const iterLanguageParts = iterLanguage.split('_');
			if (languageParts.length <= iterLanguageParts.length &&
				languageParts.every((part, index) => part === iterLanguageParts[index])) {
				languageLcid = parseInt(lcid, 10);
				break;
			}
		}
		if (languageLcid === undefined) {
			return;
		}
		// Special case: Use Spanish International.
		if (languageLcid === 1034) {
			languageLcid = 3082;
		}
		const override = debugInstallerRegKey();
		const key = reg.createKey(
			override?.hive ?? reg.HKLM,
			override?.subKey ?? 'SOFTWARE\\Windhawk',
			reg.Access.SET_VALUE | reg.Access.WOW64_32KEY);
		try {
			reg.setValueDWORD(key, 'language', languageLcid);
		} finally {
			reg.closeKey(key);
		}
	}

	private enableScheduledTask(taskName: string, enable: boolean) {
		try {
			const ps = child_process.spawn(debugSchtasksPath() ?? 'schtasks.exe', [
				'/change',
				'/tn',
				taskName,
				enable ? '/enable' : '/disable'
			]);

			let gotError = false;
			let stderr = '';

			ps.stderr.on('data', data => {
				//console.log(`ps stderr: ${data}`);
				stderr += data;
			});

			ps.on('error', err => {
				//console.log('Oh no, the error: ' + err);
				gotError = true;
				this.logger.error(err.message);
			});

			ps.on('close', code => {
				//console.log(`ps process exited with code ${code}`);
				if (!gotError && code !== 0) {
					let message = 'schtasks.exe error';
					const stderrFiltered = stderr.trim().replace(/^ERROR:\s*/, '');
					if (stderrFiltered !== '') {
						message += ': ' + stderrFiltered;
					}

					this.logger.warn(message);
				}
			});
		} catch (e) {
			this.logger.error(e.message);
		}
	}
}
