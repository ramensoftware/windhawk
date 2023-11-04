import * as child_process from 'child_process';
import * as reg from 'native-reg';
import * as path from 'path';
import * as vscode from 'vscode';
import * as ini from '../ini';

export type AppSettings = {
	language: string,
	disableUpdateCheck: boolean,
	disableRunUIScheduledTask: boolean | null,
	devModeOptOut: boolean,
	devModeUsedAtLeastOnce: boolean,
	hideTrayIcon: boolean,
	dontAutoShowToolkit: boolean,
	modTasksDialogDelay: number,
	safeMode: boolean,
	loggingVerbosity: number,
	engine: {
		loggingVerbosity: number,
		include: string[],
		exclude: string[],
		injectIntoCriticalProcesses: boolean,
		loadModsInCriticalSystemProcesses: number
	}
};

export interface AppSettingsUtils {
	getAppSettings(): AppSettings;
	updateAppSettings(appSettings: Partial<AppSettings>): void;
	shouldRestartApp(appSettings: Partial<AppSettings>): boolean;
	shouldNotifyTrayProgram(appSettings: Partial<AppSettings>): boolean;
}

export class AppSettingsUtilsPortable implements AppSettingsUtils {
	private settingsIniPath: string;
	private engineSettingsIniPath: string;

	public constructor(appDataPath: string) {
		this.settingsIniPath = path.join(appDataPath, 'settings.ini');
		this.engineSettingsIniPath = path.join(appDataPath, 'engine', 'settings.ini');
	}

	public getAppSettings() {
		const iniFileParsed = ini.fromFileOrDefault(this.settingsIniPath);
		const engineIniFileParsed = ini.fromFileOrDefault(this.engineSettingsIniPath);

		return {
			language: iniFileParsed.Settings?.Language || 'en',
			disableUpdateCheck: !!parseInt(iniFileParsed.Settings?.DisableUpdateCheck ?? '0', 10),
			disableRunUIScheduledTask: null,
			devModeOptOut: !!parseInt(iniFileParsed.Settings?.DevModeOptOut ?? '0', 10),
			devModeUsedAtLeastOnce: !!parseInt(iniFileParsed.Settings?.DevModeUsedAtLeastOnce ?? '0', 10),
			hideTrayIcon: !!parseInt(iniFileParsed.Settings?.HideTrayIcon ?? '0', 10),
			dontAutoShowToolkit: !!parseInt(iniFileParsed.Settings?.DontAutoShowToolkit ?? '0', 10),
			modTasksDialogDelay: parseInt(iniFileParsed.Settings?.ModTasksDialogDelay ?? '2000', 10),
			safeMode: !!parseInt(iniFileParsed.Settings?.SafeMode ?? '0', 10),
			loggingVerbosity: parseInt(iniFileParsed.Settings?.LoggingVerbosity ?? '0', 10),
			engine: {
				loggingVerbosity: parseInt(engineIniFileParsed.Settings?.LoggingVerbosity ?? '0', 10),
				include: (engineIniFileParsed.Settings?.Include || '').split('|'),
				exclude: (engineIniFileParsed.Settings?.Exclude || '').split('|'),
				injectIntoCriticalProcesses: !!parseInt(engineIniFileParsed.Settings?.InjectIntoCriticalProcesses ?? '0', 10),
				loadModsInCriticalSystemProcesses: parseInt(engineIniFileParsed.Settings?.LoadModsInCriticalSystemProcesses ?? '1', 10)
			}
		};
	}

	public updateAppSettings(appSettings: Partial<AppSettings>) {
		const iniFileParsed = ini.fromFileOrDefault(this.settingsIniPath);

		iniFileParsed.Settings = iniFileParsed.Settings || {};

		if (appSettings.language !== undefined) {
			iniFileParsed.Settings.Language = appSettings.language;
		}
		if (appSettings.disableUpdateCheck !== undefined) {
			iniFileParsed.Settings.DisableUpdateCheck = appSettings.disableUpdateCheck ? '1' : '0';
		}
		if (appSettings.disableRunUIScheduledTask !== undefined && appSettings.disableRunUIScheduledTask !== null) {
			throw new Error('Cannot set disableRunUIScheduledTask in portable mode');
		}
		if (appSettings.devModeOptOut !== undefined) {
			iniFileParsed.Settings.DevModeOptOut = appSettings.devModeOptOut ? '1' : '0';
		}
		if (appSettings.devModeUsedAtLeastOnce !== undefined) {
			iniFileParsed.Settings.DevModeUsedAtLeastOnce = appSettings.devModeUsedAtLeastOnce ? '1' : '0';
		}
		if (appSettings.hideTrayIcon !== undefined) {
			iniFileParsed.Settings.HideTrayIcon = appSettings.hideTrayIcon ? '1' : '0';
		}
		if (appSettings.dontAutoShowToolkit !== undefined) {
			iniFileParsed.Settings.DontAutoShowToolkit = appSettings.dontAutoShowToolkit ? '1' : '0';
		}
		if (appSettings.modTasksDialogDelay !== undefined) {
			iniFileParsed.Settings.ModTasksDialogDelay = appSettings.modTasksDialogDelay.toString();
		}
		if (appSettings.safeMode !== undefined) {
			iniFileParsed.Settings.SafeMode = appSettings.safeMode ? '1' : '0';
		}
		if (appSettings.loggingVerbosity !== undefined) {
			iniFileParsed.Settings.LoggingVerbosity = appSettings.loggingVerbosity.toString();
		}

		ini.toFile(this.settingsIniPath, iniFileParsed);

		if (appSettings.engine !== undefined) {
			const engineIniFileParsed = ini.fromFileOrDefault(this.engineSettingsIniPath);

			engineIniFileParsed.Settings = engineIniFileParsed.Settings || {};

			if (appSettings.engine.loggingVerbosity !== undefined) {
				engineIniFileParsed.Settings.LoggingVerbosity = appSettings.engine.loggingVerbosity.toString();
			}
			if (appSettings.engine.include !== undefined) {
				engineIniFileParsed.Settings.Include = appSettings.engine.include.join('|');
			}
			if (appSettings.engine.exclude !== undefined) {
				engineIniFileParsed.Settings.Exclude = appSettings.engine.exclude.join('|');
			}
			if (appSettings.engine.injectIntoCriticalProcesses !== undefined) {
				engineIniFileParsed.Settings.InjectIntoCriticalProcesses = appSettings.engine.injectIntoCriticalProcesses ? '1' : '0';
			}
			if (appSettings.engine.loadModsInCriticalSystemProcesses !== undefined) {
				engineIniFileParsed.Settings.LoadModsInCriticalSystemProcesses = appSettings.engine.loadModsInCriticalSystemProcesses.toString();
			}

			ini.toFile(this.engineSettingsIniPath, engineIniFileParsed);
		}
	}

	public shouldRestartApp(appSettings: Partial<AppSettings>) {
		return appSettings.safeMode !== undefined ||
			appSettings.loggingVerbosity !== undefined ||
			(appSettings.engine !== undefined && Object.keys(appSettings.engine).length > 0);
	}

	public shouldNotifyTrayProgram(appSettings: Partial<AppSettings>) {
		return appSettings.language !== undefined ||
			appSettings.disableUpdateCheck !== undefined ||
			appSettings.hideTrayIcon !== undefined ||
			appSettings.dontAutoShowToolkit !== undefined ||
			appSettings.modTasksDialogDelay !== undefined;
	}
}

export class AppSettingsUtilsNonPortable implements AppSettingsUtils {
	private regKey: reg.HKEY;
	private regSubKey: string;
	private engineRegSubKey: string;

	public constructor(regKey: reg.HKEY, regSubKey: string) {
		this.regKey = regKey;
		this.regSubKey = regSubKey + '\\Settings';
		this.engineRegSubKey = regSubKey + '\\Engine\\Settings';
	}

	public getAppSettings() {
		const key = reg.createKey(this.regKey, this.regSubKey,
			reg.Access.QUERY_VALUE | reg.Access.WOW64_64KEY);
		const engineKey = reg.createKey(this.regKey, this.engineRegSubKey,
			reg.Access.QUERY_VALUE | reg.Access.WOW64_64KEY);
		try {
			return {
				language: (reg.getValue(key, null, 'Language', reg.GetValueFlags.RT_REG_SZ) || 'en') as string,
				disableUpdateCheck: !!reg.getValue(key, null, 'DisableUpdateCheck', reg.GetValueFlags.RT_REG_DWORD),
				disableRunUIScheduledTask: !!reg.getValue(key, null, 'DisableRunUIScheduledTask', reg.GetValueFlags.RT_REG_DWORD),
				devModeOptOut: !!reg.getValue(key, null, 'DevModeOptOut', reg.GetValueFlags.RT_REG_DWORD),
				devModeUsedAtLeastOnce: !!reg.getValue(key, null, 'DevModeUsedAtLeastOnce', reg.GetValueFlags.RT_REG_DWORD),
				hideTrayIcon: !!reg.getValue(key, null, 'HideTrayIcon', reg.GetValueFlags.RT_REG_DWORD),
				dontAutoShowToolkit: !!reg.getValue(key, null, 'DontAutoShowToolkit', reg.GetValueFlags.RT_REG_DWORD),
				modTasksDialogDelay: (reg.getValue(key, null, 'ModTasksDialogDelay', reg.GetValueFlags.RT_REG_DWORD) ?? 2000) as number,
				safeMode: !!reg.getValue(key, null, 'SafeMode', reg.GetValueFlags.RT_REG_DWORD),
				loggingVerbosity: (reg.getValue(key, null, 'LoggingVerbosity', reg.GetValueFlags.RT_REG_DWORD) ?? 0) as number,
				engine: {
					loggingVerbosity: (reg.getValue(engineKey, null, 'LoggingVerbosity', reg.GetValueFlags.RT_REG_DWORD) ?? 0) as number,
					include: ((reg.getValue(engineKey, null, 'Include', reg.GetValueFlags.RT_REG_SZ) ?? '') as string).split('|'),
					exclude: ((reg.getValue(engineKey, null, 'Exclude', reg.GetValueFlags.RT_REG_SZ) ?? '') as string).split('|'),
					injectIntoCriticalProcesses: !!reg.getValue(engineKey, null, 'InjectIntoCriticalProcesses', reg.GetValueFlags.RT_REG_DWORD),
					loadModsInCriticalSystemProcesses: (reg.getValue(engineKey, null, 'LoadModsInCriticalSystemProcesses', reg.GetValueFlags.RT_REG_DWORD) ?? 1) as number,
				}
			};
		} finally {
			reg.closeKey(key);
			reg.closeKey(engineKey);
		}
	}

	public updateAppSettings(appSettings: Partial<AppSettings>) {
		const key = reg.createKey(this.regKey, this.regSubKey,
			reg.Access.SET_VALUE | reg.Access.WOW64_64KEY);
		try {
			if (appSettings.language !== undefined) {
				reg.setValueSZ(key, 'Language', appSettings.language);
				try {
					this.setInstallerLanguage(appSettings.language);
				} catch (e) {
					console.warn('Failed to set installer language', e);
				}
			}
			if (appSettings.disableUpdateCheck !== undefined) {
				reg.setValueDWORD(key, 'DisableUpdateCheck', appSettings.disableUpdateCheck ? 1 : 0);
				this.enableScheduledTask('WindhawkUpdateTask', !appSettings.disableUpdateCheck);
			}
			if (appSettings.disableRunUIScheduledTask !== undefined) {
				reg.setValueDWORD(key, 'DisableRunUIScheduledTask', appSettings.disableRunUIScheduledTask ? 1 : 0);
				this.enableScheduledTask('WindhawkRunUITask', !appSettings.disableRunUIScheduledTask);
			}
			if (appSettings.devModeOptOut !== undefined) {
				reg.setValueDWORD(key, 'DevModeOptOut', appSettings.devModeOptOut ? 1 : 0);
			}
			if (appSettings.devModeUsedAtLeastOnce !== undefined) {
				reg.setValueDWORD(key, 'DevModeUsedAtLeastOnce', appSettings.devModeUsedAtLeastOnce ? 1 : 0);
			}
			if (appSettings.hideTrayIcon !== undefined) {
				reg.setValueDWORD(key, 'HideTrayIcon', appSettings.hideTrayIcon ? 1 : 0);
			}
			if (appSettings.dontAutoShowToolkit !== undefined) {
				reg.setValueDWORD(key, 'DontAutoShowToolkit', appSettings.dontAutoShowToolkit ? 1 : 0);
			}
			if (appSettings.modTasksDialogDelay !== undefined) {
				reg.setValueDWORD(key, 'ModTasksDialogDelay', appSettings.modTasksDialogDelay);
			}
			if (appSettings.safeMode !== undefined) {
				reg.setValueDWORD(key, 'SafeMode', appSettings.safeMode ? 1 : 0);
			}
			if (appSettings.loggingVerbosity !== undefined) {
				reg.setValueDWORD(key, 'LoggingVerbosity', appSettings.loggingVerbosity);
			}
		} finally {
			reg.closeKey(key);
		}

		if (appSettings.engine !== undefined) {
			const engineKey = reg.createKey(this.regKey, this.engineRegSubKey,
				reg.Access.SET_VALUE | reg.Access.WOW64_64KEY);
			try {
				if (appSettings.engine.loggingVerbosity !== undefined) {
					reg.setValueDWORD(engineKey, 'LoggingVerbosity', appSettings.engine.loggingVerbosity);
				}
				if (appSettings.engine.include !== undefined) {
					reg.setValueSZ(engineKey, 'Include', appSettings.engine.include.join('|'));
				}
				if (appSettings.engine.exclude !== undefined) {
					reg.setValueSZ(engineKey, 'Exclude', appSettings.engine.exclude.join('|'));
				}
				if (appSettings.engine.injectIntoCriticalProcesses !== undefined) {
					reg.setValueDWORD(engineKey, 'InjectIntoCriticalProcesses', appSettings.engine.injectIntoCriticalProcesses ? 1 : 0);
				}
				if (appSettings.engine.loadModsInCriticalSystemProcesses !== undefined) {
					reg.setValueDWORD(engineKey, 'LoadModsInCriticalSystemProcesses', appSettings.engine.loadModsInCriticalSystemProcesses);
				}
			} finally {
				reg.closeKey(engineKey);
			}
		}
	}

	public shouldRestartApp(appSettings: Partial<AppSettings>) {
		return appSettings.safeMode !== undefined ||
			appSettings.loggingVerbosity !== undefined ||
			(appSettings.engine !== undefined && Object.keys(appSettings.engine).length > 0);
	}

	public shouldNotifyTrayProgram(appSettings: Partial<AppSettings>) {
		return appSettings.language !== undefined ||
			appSettings.disableUpdateCheck !== undefined ||
			appSettings.hideTrayIcon !== undefined ||
			appSettings.dontAutoShowToolkit !== undefined ||
			appSettings.modTasksDialogDelay !== undefined;
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
		const key = reg.createKey(reg.HKLM, 'SOFTWARE\\Windhawk',
			reg.Access.SET_VALUE | reg.Access.WOW64_32KEY);
		try {
			reg.setValueDWORD(key, 'language', languageLcid);
		} finally {
			reg.closeKey(key);
		}
	}

	private enableScheduledTask(taskName: string, enable: boolean) {
		try {
			const ps = child_process.spawn('schtasks.exe', [
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
				vscode.window.showErrorMessage(err.message);
			});

			ps.on('close', code => {
				//console.log(`ps process exited with code ${code}`);
				if (!gotError && code !== 0) {
					let message = 'schtasks.exe error';
					const stderrFiltered = stderr.trim().replace(/^ERROR:\s*/, '');
					if (stderrFiltered !== '') {
						message += ': ' + stderrFiltered;
					}

					vscode.window.showWarningMessage(message);
				}
			});
		} catch (e) {
			vscode.window.showErrorMessage(e.message);
		}
	}
}
