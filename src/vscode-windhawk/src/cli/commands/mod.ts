import type { Command } from 'commander';
import * as fs from 'fs';
import type {
	InitialSettings,
	InitialSettingsValue,
	ModConfig,
	ModMetadata,
} from '../../coreClient/contract';
import type { Environment } from '../environment';
import { ModNotInstalledError, UsageError } from '../errors';
import { parseInt32Setting } from '../intRange';
import type { CommandContext } from '../runCommand';
import { withCommand } from '../withCommand';

export function registerModCommands(program: Command): void {
	const modCmd = program.command('mod').description('Install, list, compile, and configure mods.');

	registerList(modCmd);
	registerShow(modCmd);
	registerEnable(modCmd);
	registerDisable(modCmd);
	registerRemove(modCmd);
	registerConfig(modCmd);
	registerSettings(modCmd);
	registerInstall(modCmd);
	registerUpdate(modCmd);
	registerCompile(modCmd);
}

// ---------------------------------------------------------------------------
// mod list
// ---------------------------------------------------------------------------

type ListRow = {
	id: string;
	version: string;
	name: string | null;
	author: string | null;
	description: string | null;
	enabled: boolean;
	updateAvailable: boolean;
	userRating: number;
	config: ModConfig | null;
};

function registerList(modCmd: Command): void {
	modCmd
		.command('list')
		.description('List installed mods.')
		.option('--enabled', 'Show only enabled mods.')
		.option('--disabled', 'Show only disabled mods.')
		.option('--update-available', 'Show only mods with an update available.')
		.action((cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			if (cmdOpts.enabled && cmdOpts.disabled) {
				throw new UsageError('--enabled and --disabled are mutually exclusive');
			}

			const appSettings = await env.core.getAppSettings();
			const language = appSettings.language || 'en';
			// Mirror the extension's _checkForUpdates gate: when update checks
			// are disabled, no mod reports an available update.
			const checkForUpdates = !appSettings.disableUpdateCheck;

			// syncProfile mirrors the GUI's installed-mods query: per-mod
			// version/disabled refresh plus removed-mod cleanup, persisted if
			// anything changed. Keeps the profile consistent across GUI and
			// CLI access.
			const { mods, loadErrors } = await env.core.listInstalledMods({
				language,
				checkForUpdates,
				syncProfile: true,
			});
			for (const { modId, error } of loadErrors) {
				env.logger.warn(`Failed to load metadata for mod '${modId}': ${error}`);
			}

			const allIds = Object.keys(mods);
			allIds.sort((a, b) => a.localeCompare(b));

			const rows: ListRow[] = [];
			for (const id of allIds) {
				const { metadata, config, updateAvailable, userRating } = mods[id];
				const enabled = !(config?.disabled ?? false);
				const version = metadata?.version ?? '';

				if (cmdOpts.enabled && !enabled) {
					continue;
				}
				if (cmdOpts.disabled && enabled) {
					continue;
				}
				if (cmdOpts.updateAvailable && !updateAvailable) {
					continue;
				}

				rows.push({
					id,
					version,
					name: metadata?.name ?? null,
					author: metadata?.author ?? null,
					description: metadata?.description ?? null,
					enabled,
					updateAvailable,
					userRating,
					config,
				});
			}

			output.result({ mods: rows }, () => {
				for (const row of rows) {
					const state = row.enabled ? 'enabled' : 'disabled';
					const mark = row.updateAvailable ? '\t[update]' : '';
					const name = row.name ?? '';
					process.stdout.write(`${row.id}\t${row.version}\t${state}${mark}\t${name}\n`);
				}
			});
		}));
}

// ---------------------------------------------------------------------------
// mod show
// ---------------------------------------------------------------------------

function registerShow(modCmd: Command): void {
	modCmd
		.command('show')
		.argument('<id>', 'Mod ID')
		.description('Show metadata, README, and initial settings for an installed mod.')
		.action((id: string, _cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			const config = await env.core.getModConfig(id);
			if (!config) {
				throw new ModNotInstalledError(id);
			}

			const source = await getModSourceOrThrow(env, id);

			const language = (await env.core.getAppSettings()).language || 'en';
			const parsed = await env.core.parseModSource(source, language);
			// Parse failures of any section surface as a generic failure
			// (exit 1), matching the previous direct extract* calls which
			// threw on a malformed stored source.
			if (!parsed.metadata) {
				throw new Error(parsed.errors.metadata ?? 'Failed to parse mod metadata');
			}
			if (parsed.errors.initialSettings !== undefined) {
				throw new Error(parsed.errors.initialSettings);
			}
			const { metadata, readme, initialSettings } = parsed;

			output.result(
				{ id, metadata, readme, initialSettings, config },
				() => {
					process.stdout.write(`ID:            ${id}\n`);
					process.stdout.write(`Name:          ${metadata?.name ?? ''}\n`);
					process.stdout.write(`Version:       ${metadata?.version ?? ''}\n`);
					process.stdout.write(`Author:        ${metadata?.author ?? ''}\n`);
					if (metadata?.architecture?.length) {
						process.stdout.write(`Architectures: ${metadata.architecture.join(', ')}\n`);
					}
					const enabled = !(config.disabled ?? false);
					process.stdout.write(`State:         ${enabled ? 'enabled' : 'disabled'}\n`);
					if (metadata?.description) {
						process.stdout.write('\nDescription:\n');
						process.stdout.write(indent(metadata.description, '  '));
						process.stdout.write('\n');
					}
					if (readme) {
						process.stdout.write('\nREADME:\n');
						process.stdout.write(readme);
						if (!readme.endsWith('\n')) {
							process.stdout.write('\n');
						}
					}
				},
			);
		}));
}

function indent(text: string, prefix: string): string {
	return text.split('\n').map(line => prefix + line).join('\n');
}

// Read a mod's stored source, mapping the missing-file case to
// ModNotInstalledError (exit 4) instead of surfacing a raw trace or the core's
// generic wording. The in-process backend rejects a missing source with the raw
// Node ENOENT; the DLL maps the same condition to MOD_NOT_INSTALLED. Either way
// the mod's config exists but its source file does not.
async function getModSourceOrThrow(env: Environment, id: string): Promise<string> {
	try {
		return await env.core.getModSource(id);
	} catch (e) {
		const code = (e as { code?: unknown }).code;
		if (code === 'ENOENT' || code === 'MOD_NOT_INSTALLED') {
			// Config exists but source file doesn't. Treat as not installed.
			throw new ModNotInstalledError(
				id,
				`Mod '${id}' is registered in config but its source file is missing`,
			);
		}
		throw e;
	}
}

// ---------------------------------------------------------------------------
// mod enable / mod disable
// ---------------------------------------------------------------------------

function registerEnable(modCmd: Command): void {
	modCmd
		.command('enable')
		.argument('<id>', 'Mod ID')
		.description('Enable an installed mod.')
		.action((id: string, _cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			const changed = await setModEnabledState(env, id, true);
			output.result(
				{ id, enabled: true, changed },
				() => {
					process.stdout.write(changed
						? `Enabled: ${id}\n`
						: `Already enabled: ${id}\n`);
				},
			);
		}));
}

function registerDisable(modCmd: Command): void {
	modCmd
		.command('disable')
		.argument('<id>', 'Mod ID')
		.description('Disable an installed mod.')
		.action((id: string, _cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			const changed = await setModEnabledState(env, id, false);
			output.result(
				{ id, enabled: false, changed },
				() => {
					process.stdout.write(changed
						? `Disabled: ${id}\n`
						: `Already disabled: ${id}\n`);
				},
			);
		}));
}

// CLI half of mod enable/disable: the existence check and the already-in-state
// no-op are CLI-only behavior; the actual state change is the shared core
// command (also used by the extension's enableMod IPC handler). Returns
// whether the state actually changed.
async function setModEnabledState(
	env: Environment,
	id: string,
	enable: boolean,
): Promise<boolean> {
	const config = await env.core.getModConfig(id);
	if (!config) {
		throw new ModNotInstalledError(id);
	}

	const currentlyEnabled = !(config.disabled ?? false);
	if (currentlyEnabled === enable) {
		return false;
	}

	// No tray notification: matches the extension's enableMod IPC handler,
	// which writes and lets the engine pick up the change on its own.
	await env.core.setModEnabled(id, enable);
	return true;
}

// ---------------------------------------------------------------------------
// mod remove
// ---------------------------------------------------------------------------

function registerRemove(modCmd: Command): void {
	modCmd
		.command('remove')
		.argument('<id>', 'Mod ID')
		.description('Uninstall a mod: removes config, source, DLLs, and profile entry.')
		.action((id: string, _cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			const config = await env.core.getModConfig(id);
			if (!config) {
				throw new ModNotInstalledError(id);
			}

			if (!env.globalOpts.yes) {
				// mod remove: --yes is required. Without it, print the planned
				// action and exit 2.
				process.stderr.write(
					`Would remove mod '${id}' (config, source, DLLs, profile entry). ` +
					'Pass --yes to confirm.\n',
				);
				throw new UsageError(`Refusing to remove '${id}' without --yes`);
			}

			// The extension additionally cleans editor drafts for local@ mods
			// via editorWorkspaceUtils; the CLI has no workspace concept and
			// the drafts directory is editor-mode-only, so that stays in the
			// extension handler.
			//
			// No tray notification: matches the extension's deleteMod IPC
			// handler, which writes and lets the engine pick up the change.
			await env.core.removeMod(id);

			output.result(
				{ id, removed: true },
				() => {
					process.stdout.write(`Removed: ${id}\n`);
				},
			);
		}));
}

// ---------------------------------------------------------------------------
// mod config get / mod config set
// ---------------------------------------------------------------------------

// Fields the CLI lets the user edit. It is narrower than the full ModConfig
// shape: include/exclude/architecture/ version are metadata-driven (clobbered
// on every install/compile), disabled has its own mod enable/disable commands,
// and libraryFileName is internal.
const SETTABLE_FIELDS = {
	loggingEnabled: 'boolean',
	debugLoggingEnabled: 'boolean',
	includeCustom: 'string-array',
	excludeCustom: 'string-array',
	includeExcludeCustomOnly: 'boolean',
	patternsMatchCriticalSystemProcesses: 'boolean',
} as const;

type SettableField = keyof typeof SETTABLE_FIELDS;

// Rejection messages for read-only fields, keyed by the field name that was
// attempted. Empty means the field isn't a known ModConfig key at all.
const READ_ONLY_FIELD_REASONS: Record<string, string> = {
	disabled: "use 'mod enable <id>' / 'mod disable <id>'",
	include: 'metadata-driven (overwritten on every mod install/compile)',
	exclude: 'metadata-driven (overwritten on every mod install/compile)',
	architecture: 'metadata-driven (overwritten on every mod install/compile)',
	version: 'metadata-driven',
	libraryFileName: 'internal (managed by the compiler)',
};

function registerConfig(modCmd: Command): void {
	const configCmd = modCmd.command('config').description("Read and modify a mod's configuration.");

	configCmd
		.command('get')
		.argument('<id>', 'Mod ID')
		.argument('[field]', 'Single config field to print; omit to print all')
		.description("Print a mod's config.")
		.action((id: string, field: string | undefined, _cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			const config = await env.core.getModConfig(id);
			if (!config) {
				throw new ModNotInstalledError(id);
			}

			if (field === undefined) {
				output.result({ id, config }, () => {
					for (const key of Object.keys(config).sort()) {
						const value = (config as Record<string, unknown>)[key];
						process.stdout.write(`${key}=${formatConfigValue(value)}\n`);
					}
				});
				return;
			}

			if (!(field in config)) {
				throw new UsageError(`Unknown config field '${field}'. Run 'mod config get ${id}' to see all fields.`);
			}
			const value = (config as Record<string, unknown>)[field];
			output.result({ id, field, value }, () => {
				process.stdout.write(formatConfigValue(value) + '\n');
			});
		}));

	configCmd
		.command('set')
		.argument('<id>', 'Mod ID')
		.argument('<field>', 'Config field to modify')
		.argument('[values...]', 'Value(s). One value for scalars; zero-or-more for arrays.')
		.description('Set a config field. Variadic: one value for scalars, zero-or-more for arrays.')
		.action((id: string, field: string, values: string[], _cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			const config = await env.core.getModConfig(id);
			if (!config) {
				throw new ModNotInstalledError(id);
			}

			if (field in READ_ONLY_FIELD_REASONS) {
				throw new UsageError(
					`'${field}' is not settable: ${READ_ONLY_FIELD_REASONS[field]}`,
				);
			}
			if (!(field in SETTABLE_FIELDS)) {
				throw new UsageError(
					`Unknown config field '${field}'. Settable fields: ${Object.keys(SETTABLE_FIELDS).join(', ')}.`,
				);
			}

			const settableField = field as SettableField;
			const fieldType = SETTABLE_FIELDS[settableField];
			const previousValue = (config as Record<string, unknown>)[settableField];
			const newValue = parseFieldValue(settableField, fieldType, values);

			await env.core.updateModConfig(id, { [settableField]: newValue });

			output.result(
				{ id, field, value: newValue, previousValue },
				() => {
					process.stdout.write(
						`${field}: ${formatConfigValue(previousValue)} -> ${formatConfigValue(newValue)}\n`,
					);
				},
			);
		}));
}

function parseFieldValue(field: string, type: 'boolean' | 'string-array', values: string[]): boolean | string[] {
	if (type === 'boolean') {
		if (values.length !== 1) {
			throw new UsageError(
				`Boolean field '${field}' requires exactly one value; got ${values.length}. ` +
				'Accepted: true, false, 1, 0.',
			);
		}
		const v = values[0];
		if (v === 'true' || v === '1') {
			return true;
		}
		if (v === 'false' || v === '0') {
			return false;
		}
		throw new UsageError(
			`Boolean field '${field}' value must be one of true/false/1/0; got '${v}'.`,
		);
	}
	// string-array: any count is valid (zero clears the array).
	return values;
}

function formatConfigValue(value: unknown): string {
	if (Array.isArray(value)) {
		return value.length === 0 ? '<empty list>' : value.join(', ');
	}
	return String(value);
}

// ---------------------------------------------------------------------------
// mod settings get / mod settings set
// ---------------------------------------------------------------------------

type SettingLeafType = 'boolean' | 'number' | 'string';

function registerSettings(modCmd: Command): void {
	const settingsCmd = modCmd.command('settings').description("Read and modify a mod's runtime settings.");

	settingsCmd
		.command('get')
		.argument('<id>', 'Mod ID')
		.argument('[key]', 'Single setting key to print; omit to print all')
		.description("Print a mod's runtime settings.")
		.action((id: string, key: string | undefined, _cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			const config = await env.core.getModConfig(id);
			if (!config) {
				throw new ModNotInstalledError(id);
			}
			const settings = await env.core.getModSettings(id);

			if (key === undefined) {
				output.result({ id, settings }, () => {
					for (const k of Object.keys(settings).sort()) {
						process.stdout.write(`${k}=${formatSettingValue(settings[k])}\n`);
					}
				});
				return;
			}

			const value = Object.prototype.hasOwnProperty.call(settings, key)
				? settings[key]
				: null;
			output.result({ id, key, value }, () => {
				if (value === null) {
					// Emit a blank line; scripts can detect absence by empty stdout
					// or by using --json for an unambiguous null.
					process.stdout.write('\n');
				} else {
					process.stdout.write(formatSettingValue(value) + '\n');
				}
			});
		}));

	settingsCmd
		.command('set')
		.argument('<id>', 'Mod ID')
		.argument('<key>', 'Setting key (flat-storage form, e.g. `myMod.options[0].name`)')
		.argument('<value>', 'New value')
		.description("Set a mod's runtime setting. Validates key and value type against the mod's declared initial settings.")
		.action((id: string, key: string, rawValue: string, _cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			const config = await env.core.getModConfig(id);
			if (!config) {
				throw new ModNotInstalledError(id);
			}

			const source = await getModSourceOrThrow(env, id);

			const language = (await env.core.getAppSettings()).language || 'en';
			const parsed = await env.core.parseModSource(source, language);
			// A malformed settings block in the stored source surfaces as a
			// generic failure (exit 1), matching the previous direct
			// extractInitialSettings call.
			if (parsed.errors.initialSettings !== undefined) {
				throw new Error(parsed.errors.initialSettings);
			}
			const initialSettings = parsed.initialSettings;
			if (!initialSettings || initialSettings.length === 0) {
				throw new UsageError(`Mod '${id}' declares no settings; there is nothing to set.`);
			}

			const keyTypes = flattenSettingKeyTypes(initialSettings);
			const declaredType = keyTypes[key];
			if (declaredType === undefined) {
				const validKeys = Object.keys(keyTypes).sort();
				throw new UsageError(
					`Key '${key}' is not a declared setting of mod '${id}'.\nValid keys:\n  ${validKeys.join('\n  ')}`,
				);
			}

			const newValue = parseSettingInput(key, declaredType, rawValue);

			const current = await env.core.getModSettings(id);
			const previousValue = Object.prototype.hasOwnProperty.call(current, key)
				? current[key]
				: null;

			await env.core.setModSettings(id, { ...current, [key]: newValue });

			// No tray notification: matches the extension's setModSettings IPC
			// handler. The engine picks up setting changes automatically.

			output.result(
				{ id, key, value: newValue, previousValue },
				() => {
					process.stdout.write(
						`${key}: ${formatSettingValue(previousValue)} -> ${formatSettingValue(newValue)}\n`,
					);
				},
			);
		}));
}

// Flatten a mod's declared initial settings into a flat { key: typeTag } map
// matching the engine's storage key convention (scalar=`key`, nested
// object=`parent.child`, array of scalars=`parent[0]`, array of
// objects=`parent[0].child`). Unlike the engine-side flattening, this preserves
// boolean-vs-number distinction so `set` can type-check input.
function flattenSettingKeyTypes(
	settings: InitialSettings,
	prefix: string = '',
): Record<string, SettingLeafType> {
	const out: Record<string, SettingLeafType> = {};
	for (const item of settings) {
		const key = prefix ? `${prefix}.${item.key}` : item.key;
		flattenSettingValue(item.value, key, out);
	}
	return out;
}

function flattenSettingValue(
	value: InitialSettingsValue,
	key: string,
	out: Record<string, SettingLeafType>,
): void {
	if (typeof value === 'boolean') {
		out[key] = 'boolean';
		return;
	}
	if (typeof value === 'number') {
		out[key] = 'number';
		return;
	}
	if (typeof value === 'string') {
		out[key] = 'string';
		return;
	}
	if (Array.isArray(value)) {
		if (value.length === 0) {
			// Empty array in the source: no type info to validate against; skip.
			return;
		}
		const first = value[0];
		if (typeof first === 'number' || typeof first === 'string' || typeof first === 'boolean') {
			// Array of primitives: each index is a leaf of the same type.
			for (let i = 0; i < value.length; i++) {
				flattenSettingValue(value[i] as InitialSettingsValue, `${key}[${i}]`, out);
			}
		} else if (Array.isArray(first)) {
			// InitialSettings[]: an array where each element is itself a
			// nested InitialSettings (array of items). Each top-level index
			// is a separate grouped object; recurse into each.
			for (let i = 0; i < value.length; i++) {
				flattenSettingKeyTypesInto(value[i] as InitialSettings, `${key}[${i}]`, out);
			}
		} else {
			// InitialSettings: this value is a nested object whose leaves
			// live at the current key's namespace (not per-index).
			// `first` is an InitialSettingItem { key, value }.
			flattenSettingKeyTypesInto(value as InitialSettings, key, out);
		}
		return;
	}
}

function flattenSettingKeyTypesInto(
	settings: InitialSettings,
	prefix: string,
	out: Record<string, SettingLeafType>,
): void {
	for (const item of settings) {
		const key = `${prefix}.${item.key}`;
		flattenSettingValue(item.value, key, out);
	}
}

function parseSettingInput(
	key: string,
	type: SettingLeafType,
	raw: string,
): string | number {
	if (type === 'boolean') {
		if (raw === 'true' || raw === '1') {
			return 1;
		}
		if (raw === 'false' || raw === '0') {
			return 0;
		}
		throw new UsageError(
			`Setting '${key}' is declared as boolean; value must be one of true/false/1/0, got '${raw}'.`,
		);
	}
	if (type === 'number') {
		return parseInt32Setting(key, raw);
	}
	// string
	return raw;
}

function formatSettingValue(value: string | number | null): string {
	if (value === null) {
		return '<unset>';
	}
	return String(value);
}

// ---------------------------------------------------------------------------
// mod install
// ---------------------------------------------------------------------------

function registerInstall(modCmd: Command): void {
	modCmd
		.command('install')
		.argument('[id]', 'Mod ID (required unless --file is used; with --file, optional sanity-check against source metadata)')
		.argument('[version]', 'Mod version. Default is latest. Ignored with --file.')
		.description('Install or reinstall a mod from the repository or a local source file.')
		.option('--file <path>', "Read mod source from a local file. Use '-' for stdin.")
		.option('--disabled', 'Install in disabled state. Default is enabled.')
		.option('--no-precompiled', 'Force local compilation even if alwaysCompileModsLocally is false.')
		.action((
			idArg: string | undefined,
			versionArg: string | undefined,
			cmdOpts: { file?: string; disabled?: boolean; precompiled: boolean },
			cmd,
		) => withCommand(cmd, async (ctx) => {
			const { env, output } = ctx;
			const fileMode = cmdOpts.file !== undefined;

			if (!fileMode && !idArg) {
				throw new UsageError('mod install: provide <id> or --file <path>');
			}
			if (fileMode && versionArg !== undefined) {
				throw new UsageError('mod install: [version] is not valid with --file');
			}
			if (fileMode && cmdOpts.precompiled === false) {
				// --file always compiles locally (a local mod has no repo DLL to
				// download), so --no-precompiled is a no-op. Reject it rather than
				// silently ignore, matching the [version] rule above.
				throw new UsageError('mod install: --no-precompiled has no effect with --file (it always compiles locally)');
			}

			const rawSource = fileMode
				? readFileOrStdin(cmdOpts.file!)
				: await fetchRepoSource(env, idArg!, versionArg);

			const { modId, normalizedSource, metadata } = await extractAndReconcile(env, rawSource, idArg);

			// A --file install is a locally-authored mod: store it under
			// `local@<id>`, like the UI editor's compileEditedMod. This keeps it
			// out of the repo-mod namespace (so it can't clobber a repo mod with
			// the same id) and out of the user profile - runInstallPipeline's
			// `local@` guard skips the profile write.
			const installId = fileMode ? `local@${modId}` : modId;

			const result = await runInstallPipeline(ctx, installId, normalizedSource, metadata, {
				disabled: cmdOpts.disabled === true,
				// --file always compiles locally: the supplied source is
				// authoritative, so downloading a precompiled DLL by id/version
				// would install something other than what was provided (or 404 if
				// the mod isn't in the repo).
				forceLocalCompile: fileMode || !cmdOpts.precompiled,
			});

			output.result(
				{
					id: installId,
					version: result.modVersion,
					metadata,
					config: result.config,
					architectures: result.architecture,
					compiledLocally: result.compiledLocally,
				},
				() => {
					const verb = fileMode ? 'Installed from file' : 'Installed';
					// Reflect the actual persisted state, not the flag: a reinstall
					// without --disabled preserves an existing disabled mod, so
					// cmdOpts.disabled would under-report it (matches `mod update`).
					const disabledMarker = result.config.disabled ? ' [disabled]' : '';
					process.stdout.write(`${verb}: ${installId} ${result.modVersion}${disabledMarker}\n`);
					process.stdout.write(
						`Method:       ${result.compiledLocally ? 'compiled locally' : 'downloaded precompiled'}\n`,
					);
					if (result.architecture.length) {
						process.stdout.write(`Architectures: ${result.architecture.join(', ')}\n`);
					}
				},
			);
		}));
}

// Reads a source file from disk, or from stdin when path is '-'. Synchronous
// on stdin matches the extension's single-shot install ergonomics and keeps
// the install flow a straight await chain.
function readFileOrStdin(filePath: string): string {
	if (filePath === '-') {
		return fs.readFileSync(0, 'utf8');
	}
	try {
		return fs.readFileSync(filePath, 'utf8');
	} catch (e) {
		// A missing --file path is a bad flag value, not an unhandled
		// exception; rewrap so the output adapter maps it to exit 2 (USAGE)
		// instead of exit 1 (GENERIC).
		if ((e as NodeJS.ErrnoException).code === 'ENOENT') {
			throw new UsageError(`--file: '${filePath}' does not exist`);
		}
		throw e;
	}
}

// Fetch a mod's source from the public repo. Emits a stderr progress line
// unless --quiet is set. 404 -> ModNotInRepoError (exit 5); other network
// failures -> RepoUnreachableError (exit 6), per the core's repository
// client.
async function fetchRepoSource(
	env: Environment,
	modId: string,
	version: string | undefined,
): Promise<string> {
	if (!env.globalOpts.quiet) {
		process.stderr.write(
			`Fetching ${modId}${version ? ` version ${version}` : ''} from repository...\n`,
		);
	}
	return env.core.fetchRepoModSource(modId, version);
}

// Normalize line endings, extract metadata, and check that the source's
// declared id matches the supplied `idArg` if any. Returns the normalized
// source (suitable for writing back to disk) alongside the resolved mod id
// and parsed metadata. Throws UsageError on any id mismatch.
async function extractAndReconcile(
	env: Environment,
	source: string,
	idArg: string | undefined,
): Promise<{
	modId: string;
	normalizedSource: string;
	metadata: ModMetadata;
}> {
	// Repo fetches arrive normalized from the core, but --file/stdin sources
	// don't; normalize here (idempotent) so source-on-disk stays consistent
	// with the GUI install.
	const normalizedSource = source.replace(/\r\n|\r|\n/g, '\r\n');
	const language = (await env.core.getAppSettings()).language || 'en';
	// Metadata parsing validates the source (metadata block present,
	// parseable lines, valid @id / @architecture) and reports any violation -
	// including a missing @id. A malformed source being installed is a usage
	// error (exit 2, "bad value"), not an internal one.
	const parsed = await env.core.parseModSource(normalizedSource, language);
	if (!parsed.metadata) {
		throw new UsageError(parsed.errors.metadata ?? 'Failed to parse mod metadata');
	}
	const metadata = parsed.metadata;
	const sourceModId = metadata.id;
	if (!sourceModId) {
		throw new UsageError('Mod id must be specified in the source code (no `// @id`).');
	}
	if (idArg && sourceModId !== idArg) {
		throw new UsageError(
			`Mod id mismatch: source declares '${sourceModId}', argument was '${idArg}'.`,
		);
	}
	return { modId: sourceModId, normalizedSource, metadata };
}

type InstallPipelineOpts = {
	// true => set disabled=true on install. undefined/false => leave disabled
	// field out of the patch, which preserves existing state for reinstalls
	// (matching `mod update`) and lets the backend default to enabled on a
	// brand-new install (matching the extension's installMod IPC handler).
	disabled: boolean;
	forceLocalCompile: boolean;
};

type InstallPipelineResult = {
	modVersion: string;
	architecture: string[];
	compiledLocally: boolean;
	targetDllName: string;
	config: ModConfig;
};

// Shared tail of `mod install` and `mod update`, CLI side: decide
// compile-vs-download, emit stderr progress, and hand off to the core's
// installMod operation (settings migration, compile-or-download, persist
// config/source/user-profile, clean up stale DLLs). `source` must be
// CRLF-normalized and `metadata` must already have been reconciled with
// `modId`. The operation is tracked so Ctrl+C cancels an in-flight compile.
//
// No tray notification: matches the extension's installMod IPC handler.
async function runInstallPipeline(
	ctx: CommandContext,
	modId: string,
	source: string,
	metadata: ModMetadata,
	opts: InstallPipelineOpts,
): Promise<InstallPipelineResult> {
	const { env } = ctx;
	const modVersion = metadata.version || '';
	const architecture = metadata.architecture || [];

	// Compile-vs-download decision (matches installMod IPC handler, except
	// that the GUI uses its cached alwaysCompileModsLocally value while the
	// CLI reads the setting fresh).
	const appSettings = await env.core.getAppSettings();
	const compileLocally = appSettings.alwaysCompileModsLocally || opts.forceLocalCompile;

	if (compileLocally && !env.globalOpts.quiet) {
		for (const arch of architecture.length ? architecture : ['x86', 'x86-64']) {
			process.stderr.write(`Compiling for ${arch}...\n`);
		}
	}

	const result = await ctx.track(env.core.installMod({
		storageId: modId,
		source,
		metadata,
		// See InstallPipelineOpts: undefined preserves existing state.
		disabled: opts.disabled ? true : undefined,
		compileLocally,
		// local@ mods (file installs) stay out of the user profile.
		trackInProfile: !modId.startsWith('local@'),
	}));

	return {
		modVersion,
		architecture,
		compiledLocally: compileLocally,
		targetDllName: result.targetDllName,
		config: result.config,
	};
}

// ---------------------------------------------------------------------------
// mod update
// ---------------------------------------------------------------------------

function registerUpdate(modCmd: Command): void {
	modCmd
		.command('update')
		.argument('<id>', 'Mod ID')
		.description('Update an installed mod to its latest repository version.')
		.option('--disabled', 'Install in disabled state. Without this flag, the current state is preserved.')
		.option('--no-precompiled', 'Force local compilation even if alwaysCompileModsLocally is false.')
		.action((
			id: string,
			cmdOpts: { disabled?: boolean; precompiled: boolean },
			cmd,
		) => withCommand(cmd, async (ctx) => {
			const { env, output } = ctx;
			const currentConfig = await env.core.getModConfig(id);
			if (!currentConfig) {
				throw new ModNotInstalledError(id);
			}
			const previousVersion = currentConfig.version || '';

			const rawSource = await fetchRepoSource(env, id, undefined);
			const { modId, normalizedSource, metadata } = await extractAndReconcile(env, rawSource, id);
			const latestVersion = metadata.version || '';

			// Fast path: latest == installed. No changes, no write. Matches
			// the spec'd `data.upToDate: true` exit-0 behavior.
			if (latestVersion && latestVersion === previousVersion) {
				output.result(
					{
						id: modId,
						version: latestVersion,
						metadata,
						config: currentConfig,
						architectures: metadata.architecture || [],
						compiledLocally: false,
						upToDate: true,
						previousVersion,
					},
					() => {
						process.stdout.write(`Already up to date: ${modId} ${latestVersion}\n`);
					},
				);
				return;
			}

			const result = await runInstallPipeline(ctx, modId, normalizedSource, metadata, {
				// Preserve current disabled state unless --disabled is passed.
				// The pipeline's `disabled: false` is a no-op on reinstalls -
				// it simply omits the field from the config patch, so the
				// existing value survives.
				disabled: cmdOpts.disabled === true,
				forceLocalCompile: !cmdOpts.precompiled,
			});

			output.result(
				{
					id: modId,
					version: result.modVersion,
					metadata,
					config: result.config,
					architectures: result.architecture,
					compiledLocally: result.compiledLocally,
					upToDate: false,
					previousVersion,
				},
				() => {
					const disabledMarker = result.config.disabled ? ' [disabled]' : '';
					process.stdout.write(
						`Updated: ${modId} ${previousVersion} -> ${result.modVersion}${disabledMarker}\n`,
					);
					process.stdout.write(
						`Method:       ${result.compiledLocally ? 'compiled locally' : 'downloaded precompiled'}\n`,
					);
					if (result.architecture.length) {
						process.stdout.write(`Architectures: ${result.architecture.join(', ')}\n`);
					}
				},
			);
		}));
}

// ---------------------------------------------------------------------------
// mod compile
// ---------------------------------------------------------------------------

function registerCompile(modCmd: Command): void {
	modCmd
		.command('compile')
		.argument('<id>', 'Mod ID')
		.description('Recompile an already-installed mod from its stored source.')
		.action((id: string, _cmdOpts, cmd) => withCommand(cmd, async (ctx) => {
			const { env, output } = ctx;
			const currentConfig = await env.core.getModConfig(id);
			if (!currentConfig) {
				throw new ModNotInstalledError(id);
			}

			const source = await getModSourceOrThrow(env, id);

			const language = (await env.core.getAppSettings()).language || 'en';
			// As in extractAndReconcile: a malformed stored source (including a
			// missing @id) is a usage error (exit 2) rather than GENERIC.
			const parsed = await env.core.parseModSource(source, language);
			if (!parsed.metadata) {
				throw new UsageError(parsed.errors.metadata ?? 'Failed to parse mod metadata');
			}
			const metadata = parsed.metadata;
			const sourceModId = metadata.id;
			if (!sourceModId) {
				throw new UsageError('Mod id must be specified in the source code (no `// @id`).');
			}
			// Local mods are stored under `local@<id>` but the source still
			// declares the bare `<id>` in its metadata; strip the prefix before
			// comparing, matching the extension's compileMod IPC handler.
			const expectedSourceModId = id.replace(/^local@/, '');
			if (sourceModId !== expectedSourceModId) {
				throw new UsageError(
					`Mod id mismatch: source declares '${sourceModId}', config has '${id}'.`,
				);
			}

			const modVersion = metadata.version || '';
			const architecture = metadata.architecture || [];

			if (!env.globalOpts.quiet) {
				for (const arch of architecture.length ? architecture : ['x86', 'x86-64']) {
					process.stderr.write(`Compiling for ${arch}...\n`);
				}
			}

			// No tray notification; the engine picks up the new DLL on its next
			// mod dispatch.
			const result = await ctx.track(env.core.compileInstalledMod({
				storageId: id,
				source,
				metadata,
			}));

			output.result(
				{
					id,
					version: modVersion,
					metadata,
					config: result.config,
					architectures: architecture,
				},
				() => {
					process.stdout.write(`Compiled: ${id} ${modVersion}\n`);
					if (architecture.length) {
						process.stdout.write(`Architectures: ${architecture.join(', ')}\n`);
					}
				},
			);
		}));
}
