import * as fs from 'fs';
import * as yaml from 'js-yaml';
import * as jsonschema from 'jsonschema';
import * as path from 'path';

const modMetadataParams = {
	singleValue: [
		'id',
		'version',
		'github',
		'twitter',
		'homepage',
		'compilerOptions',
		'license',
		'donateUrl',
	],
	singleValueLocalizable: [
		'name',
		'description',
		'author',
	],
	multiValue: [
		'include',
		'exclude',
		'architecture',
	],
} as const;

type ModMetadataParamsSingleValue = typeof modMetadataParams.singleValue[number];
type ModMetadataParamsSingleValueLocalizable = typeof modMetadataParams.singleValueLocalizable[number];
type ModMetadataParamsMultiValue = typeof modMetadataParams.multiValue[number];

function isModMetadataParamsSingleValue(k: string): k is ModMetadataParamsSingleValue {
	return modMetadataParams.singleValue.includes(k as any);
}
function isModMetadataParamsSingleValueLocalizable(k: string): k is ModMetadataParamsSingleValueLocalizable {
	return modMetadataParams.singleValueLocalizable.includes(k as any);
}
function isModMetadataParamsMultiValue(k: string): k is ModMetadataParamsMultiValue {
	return modMetadataParams.multiValue.includes(k as any);
}

type ModMetadata = Partial<
	Record<ModMetadataParamsSingleValue, string> &
	Record<ModMetadataParamsSingleValueLocalizable, string> &
	Record<ModMetadataParamsMultiValue, string[]>
>;

export default class ModSourceUtils {
	private modsSourcePath: string;

	public constructor(appDataPath: string) {
		this.modsSourcePath = path.join(appDataPath, 'ModsSource');
	}

	private getModSourcePath(modId: string) {
		return path.join(this.modsSourcePath, modId + '.wh.cpp');
	}

	private getBestLanguageMatch(matchLanguage: string, candidates: {
		language: string | null,
		value: string
	}[]) {
		const languages = candidates.map(x => x.language && x.language.toLowerCase());

		let iterLanguage = matchLanguage;
		let foundIndex;

		for (; ;) {
			// Exact match.
			foundIndex = languages.indexOf(iterLanguage);
			if (foundIndex !== -1) {
				return candidates[foundIndex];
			}

			// A more specific language.
			foundIndex = languages.findIndex(language => language && language.startsWith(iterLanguage));
			if (foundIndex !== -1) {
				return candidates[foundIndex];
			}

			if (!iterLanguage.includes('-')) {
				break;
			}

			iterLanguage = iterLanguage.replace(/-[^-]*$/, '');
		}

		// No language.
		foundIndex = languages.indexOf(null);
		if (foundIndex !== -1) {
			return candidates[foundIndex];
		}

		// No matches of any kind, return the first item.
		return candidates[0];
	}

	private extractMetadataRaw(modSource: string) {
		const metadataBlockMatch = modSource.match(/^\/\/[ \t]+==WindhawkMod==[ \t]*$([\s\S]+?)^\/\/[ \t]+==\/WindhawkMod==[ \t]*$/m);
		if (!metadataBlockMatch) {
			throw new Error('Couldn\'t find a metadata block in the source code');
		}

		const metadataBlock = metadataBlockMatch[1];

		const result: Record<string, {
			language: string | null,
			value: string
		}[]> = {};

		for (const line of metadataBlock.split('\n')) {
			const lineTrimmed = line.trimEnd();
			if (lineTrimmed === '') {
				continue;
			}

			const match = lineTrimmed.match(/^\/\/[ \t]+@(_?[a-zA-Z]+)(?::([a-z]{2}(?:-[A-Z]{2})?))?[ \t]+(.*)$/);
			if (!match) {
				const lineTruncated = lineTrimmed.length > 20 ? (lineTrimmed.slice(0, 17) + '...') : lineTrimmed;
				throw new Error('Couldn\'t parse metadata line: ' + lineTruncated);
			}

			const key = match[1];
			const language = match[2] as string | undefined;
			const value = match[3];

			result[key] = result[key] ?? [];
			result[key].push({
				language: language ?? null,
				value
			});
		}

		return result;
	}

	private validateMetadata(metadata: ModMetadata) {
		const modId = metadata.id;
		if (!modId) {
			throw new Error('Mod id must be specified in the source code');
		}

		if (!modId.match(/^[0-9a-z-]+$/)) {
			throw new Error('Mod id must only contain the following characters: 0-9, a-z, and a hyphen (-)');
		}

		const paths = {
			include: metadata.include,
			exclude: metadata.exclude
		};
		for (const [category, pathsArray] of Object.entries(paths)) {
			for (const path of pathsArray || []) {
				if (path.match(/[/"<>|]/)) {
					throw new Error(`Mod ${category} path contains one of the forbidden characters: / " < > |`);
				}
			}
		}

		const supportedArchitecture = [
			'x86',
			'x86-64',
			'amd64',
			'arm64'
		];
		for (const architecture of metadata.architecture || []) {
			if (!supportedArchitecture.includes(architecture)) {
				throw new Error(`Mod architecture must be one of ${supportedArchitecture.join(', ')}: ${architecture}`);
			}
		}
	}

	public extractMetadata(modSource: string, language: string) {
		const metadataRaw = this.extractMetadataRaw(modSource);

		const result: ModMetadata = {};

		for (const [metadataKeyRaw, metadataValue] of Object.entries(metadataRaw)) {
			if (metadataValue.length === 0) {
				throw new Error(`Missing metadata parameter: ${metadataKeyRaw}`);
			}

			const metadataKey = metadataKeyRaw.replace(/^_/, '');

			if (isModMetadataParamsSingleValueLocalizable(metadataKey)) {
				const languages = new Set<string | null>();
				for (const item of metadataValue) {
					if (languages.has(item.language)) {
						throw new Error(`Duplicate metadata parameter: ${metadataKey}` + (item.language !== null ? `:${item.language}` : ''));
					}

					languages.add(item.language);
				}

				result[metadataKey] = this.getBestLanguageMatch(language, metadataValue).value;
			} else if (isModMetadataParamsMultiValue(metadataKey)) {
				for (const item of metadataValue) {
					if (item.language !== null) {
						throw new Error(`Metadata parameter can't be localized: ${metadataKey}:${item.language}`);
					}
				}

				result[metadataKey] = metadataValue.map(x => x.value);
			} else if (isModMetadataParamsSingleValue(metadataKey)) {
				for (const item of metadataValue) {
					if (item.language !== null) {
						throw new Error(`Metadata parameter can't be localized: ${metadataKey}:${item.language}`);
					}
				}

				if (metadataValue.length > 1) {
					throw new Error(`Duplicate metadata parameter: ${metadataKey}`);
				}

				result[metadataKey] = metadataValue[0].value;
			} else if (metadataKeyRaw.startsWith('_')) {
				// Ignore for forward compatibility.
			} else {
				throw new Error(`Unsupported metadata parameter: ${metadataKey}`);
			}
		}

		this.validateMetadata(result);
		return result;
	}

	public appendToIdAndName(modSource: string, appendToId?: string, appendToName?: string) {
		// This function can be made more generic in the future, if necessary.
		const search = /(^\/\/[ \t]+==WindhawkMod==[ \t]*$)([\s\S]+?)(^\/\/[ \t]+==\/WindhawkMod==[ \t]*$)/m;
		return modSource.replace(search, (match, p1: string, p2: string, p3: string) => {
			let p2New = p2;

			if (appendToId) {
				p2New = p2New.replace(/^(\/\/[ \t]+@id[ \t]+)(.*?)([ \t]*)$/m,
					'$1$2' + appendToId.replace(/\$/g, '$$$$') + '$3');
			}

			if (appendToName) {
				p2New = p2New.replace(/^(\/\/[ \t]+@name(?::(?:[a-z]{2}(?:-[A-Z]{2})?))?[ \t]+)(.*?)([ \t]*)$/mg,
					'$1$2' + appendToName.replace(/\$/g, '$$$$') + '$3');
			}

			return p1 + p2New + p3;
		});
	}

	public getMetadataOfInstalled(language: string, onLoadError: (modId: string, error: Error) => void) {
		const mods: Record<string, ModMetadata> = {};

		let modsSourceDir: fs.Dir;
		try {
			modsSourceDir = fs.opendirSync(this.modsSourcePath);
		} catch (e) {
			// Ignore if file doesn't exist.
			if (e.code !== 'ENOENT') {
				throw e;
			}
			return mods;
		}

		try {
			let modsSourceDirEntry: fs.Dirent | null;
			while ((modsSourceDirEntry = modsSourceDir.readSync()) !== null) {
				if (modsSourceDirEntry.isFile() && modsSourceDirEntry.name.endsWith('.wh.cpp')) {
					const modId = modsSourceDirEntry.name.slice(0, -'.wh.cpp'.length);
					const modSourcePath = path.join(this.modsSourcePath, modsSourceDirEntry.name);
					try {
						const modSourceMetadata = this.extractMetadata(fs.readFileSync(modSourcePath, 'utf8'), language);
						mods[modId] = modSourceMetadata;
					} catch (e) {
						onLoadError(modId, e);
					}
				}
			}
		} finally {
			modsSourceDir.closeSync();
		}

		return mods;
	}

	public getSource(modId: string) {
		const modSourcePath = this.getModSourcePath(modId);
		return fs.readFileSync(modSourcePath, 'utf8');
	}

	public setSource(modId: string, modSource: string) {
		const modSourcePath = this.getModSourcePath(modId);
		fs.mkdirSync(path.dirname(modSourcePath), { recursive: true });
		fs.writeFileSync(modSourcePath, modSource);
	}

	public doesSourceExist(modId: string) {
		const modSourcePath = this.getModSourcePath(modId);
		return fs.existsSync(modSourcePath);
	}

	public extractReadme(modSource: string) {
		const readmeBlockMatch = modSource.match(/^\/\/[ \t]+==WindhawkModReadme==[ \t]*$\s*\/\*\s*([\s\S]+?)\s*\*\/\s*^\/\/[ \t]+==\/WindhawkModReadme==[ \t]*$/m);
		if (readmeBlockMatch === null) {
			return null;
		}

		return readmeBlockMatch[1];
	}

	private extractInitialSettingsRaw(modSource: string) {
		const settingsBlockMatch = modSource.match(/^\/\/[ \t]+==WindhawkModSettings==[ \t]*$\s*\/\*\s*([\s\S]+?)\s*\*\/\s*^\/\/[ \t]+==\/WindhawkModSettings==[ \t]*$/m);
		if (settingsBlockMatch === null) {
			return null;
		}

		const settings = yaml.load(settingsBlockMatch[1], {
			schema: yaml.JSON_SCHEMA
		});
		if (!Array.isArray(settings)) {
			throw new Error('Failed to parse settings: not a valid YAML array');
		}

		const schema = {
			"type": "array",
			"minItems": 1,
			"items": {
				"type": "object",
				"minProperties": 1,
				"additionalProperties": false,
				"patternProperties": {
					"^[0-9A-Za-z_-]+$": {
						"anyOf": [
							{ "type": "boolean" },
							{ "type": "number" },
							{ "type": "string" },
							{ "$ref": "#" },
							{
								"type": "array",
								"minItems": 1,
								"anyOf": [
									{ "items": { "type": "number" } },
									{ "items": { "type": "string" } },
									{ "items": { "$ref": "#" } }
								]
							}
						]
					},
					"^\\$(name|description)(:[a-z]{2}(-[A-Z]{2})?)?$": {
						"type": "string"
					},
					"^\\$(options)(:[a-z]{2}(-[A-Z]{2})?)?$": {
						"type": "array",
						"minItems": 2,
						"items": {
							"type": "object",
							"minProperties": 1,
							"maxProperties": 1,
							"patternProperties": {
								"^.*$": {
									"type": "string"
								}
							}
						}
					}
				}
			}
		};

		const validatorResult = jsonschema.validate(settings, schema);
		if (!validatorResult.valid) {
			throw new Error('Failed to parse settings: ' + validatorResult.toString());
		}

		return settings as Record<string, any>[];
	}

	public extractInitialSettings(modSource: string, language: string) {
		const parseSettings = (settings: Record<string, any>[]) => {
			return settings.map(parseSettingsValueAnnotated);
		};

		const parseSettingsValueAnnotated = (value: Record<string, any>) => {
			const actualParameters = Object.keys(value).filter(x => !x.startsWith('$'));
			if (actualParameters.length === 0) {
				throw new Error('Missing settings key');
			} else if (actualParameters.length > 1) {
				throw new Error('More than one settings key');
			}

			const actualParameter = actualParameters[0];
			const metaParameters = Object.keys(value).filter(x => x.startsWith('$'));

			const result: Record<string, any> = {};

			for (const paramWithPrefix of metaParameters) {
				const param = paramWithPrefix.slice(1); // remove '$'
				const paramParts = param.split(':');

				result[paramParts[0]] = result[paramParts[0]] ?? [];
				result[paramParts[0]].push({
					language: paramParts[1] ?? null,
					value: value[paramWithPrefix]
				});
			}

			for (const key of Object.keys(result)) {
				result[key] = this.getBestLanguageMatch(language, result[key]).value;
			}

			result.key = actualParameter;
			result.value = parseSettingsValue(value[actualParameter]);

			return result;
		};

		const parseSettingsValue = (value: any): any => {
			if (typeof value === 'boolean' ||
				typeof value === 'number' ||
				typeof value === 'string' ||
				typeof value[0] === 'number' ||
				typeof value[0] === 'string') {
				return value;
			}

			return Array.isArray(value[0]) ? value.map(parseSettings) : parseSettings(value);
		};

		const settingsRaw = this.extractInitialSettingsRaw(modSource);
		if (!settingsRaw) {
			return null;
		}

		return parseSettings(settingsRaw);
	}

	public extractInitialSettingsForEngine(modSource: string) {
		const parseSettings = (settings: Record<string, any>[], keyPrefix = '') => {
			for (const value of settings) {
				parseSettingsValueAnnotated(value, keyPrefix);
			}
		};

		const parseSettingsValueAnnotated = (value: Record<string, any>, keyPrefix = '') => {
			const actualParameters = Object.keys(value).filter(x => !x.startsWith('$'));
			if (actualParameters.length === 0) {
				throw new Error('Missing settings key');
			} else if (actualParameters.length > 1) {
				throw new Error('More than one settings key');
			}

			const actualParameter = actualParameters[0];

			const key = (keyPrefix && (keyPrefix + '.')) + actualParameter;
			parseSettingsValue(value[actualParameter], key);
		};

		const parseSettingsValue = (value: any, key: string) => {
			if (typeof value === 'boolean') {
				parsed[key] = value ? 1 : 0;
				return;
			}

			if (typeof value === 'number' ||
				typeof value === 'string') {
				parsed[key] = value;
				return;
			}

			if (typeof value[0] === 'number' ||
				typeof value[0] === 'string') {
				for (const [i, item] of value.entries()) {
					parseSettingsValue(item, `${key}[${i}]`);
				}
				return;
			}

			if (Array.isArray(value[0])) {
				for (const [i, item] of value.entries()) {
					parseSettings(item, `${key}[${i}]`);
				}
			} else {
				parseSettings(value, key);
			}
		};

		const parsed: Record<string, string | number> = {};

		const settingsRaw = this.extractInitialSettingsRaw(modSource);
		if (!settingsRaw) {
			return null;
		}

		parseSettings(settingsRaw);
		return parsed;
	}

	public deleteSource(modId: string) {
		const modSourcePath = this.getModSourcePath(modId);
		try {
			fs.unlinkSync(modSourcePath);
		} catch (e) {
			// Ignore if file doesn't exist.
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}
	}
}
