import type { Command } from 'commander';
import type { AppSettings } from '../../coreClient/contract';
import { RestartRequiredError, UsageError } from '../errors';
import { parseInt32Setting } from '../intRange';
import { withCommand } from '../withCommand';

type FieldType = 'string' | 'boolean' | 'number' | 'string-array';

// Dotted-key schema for AppSettings. Keys map directly to the user-facing
// `app settings` namespace: top-level names for flat settings, `engine.<x>`
// for fields nested under the engine sub-object.
const SCHEMA: Record<string, FieldType> = {
	language: 'string',
	disableUpdateCheck: 'boolean',
	// May throw at write time in portable mode; service rejects there.
	disableRunUIScheduledTask: 'boolean',
	devModeOptOut: 'boolean',
	hideTrayIcon: 'boolean',
	alwaysCompileModsLocally: 'boolean',
	dontAutoShowToolkit: 'boolean',
	modTasksDialogDelay: 'number',
	safeMode: 'boolean',
	loggingVerbosity: 'number',
	'engine.loggingVerbosity': 'number',
	'engine.include': 'string-array',
	'engine.exclude': 'string-array',
	'engine.injectIntoCriticalProcesses': 'boolean',
	'engine.injectIntoIncompatiblePrograms': 'boolean',
	'engine.injectIntoGames': 'boolean',
};

export function registerAppCommands(program: Command): void {
	const appCmd = program.command('app').description('Read and modify Windhawk application-level settings.');
	const settingsCmd = appCmd.command('settings').description('Application settings.');

	registerGet(settingsCmd);
	registerSet(settingsCmd);
}

// ---------------------------------------------------------------------------
// app settings get
// ---------------------------------------------------------------------------

function registerGet(settingsCmd: Command): void {
	settingsCmd
		.command('get')
		.argument('[key]', 'Single setting key (dotted for nested, e.g. `engine.injectIntoGames`); omit to print all')
		.description('Print Windhawk application settings.')
		.action((key: string | undefined, _cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			const settings = await env.core.getAppSettings();

			if (key === undefined) {
				output.result({ settings }, () => {
					for (const line of formatFlatSettings(settings)) {
						process.stdout.write(line + '\n');
					}
				});
				return;
			}

			if (!(key in SCHEMA)) {
				throw new UsageError(
					`Unknown app setting '${key}'. Run 'app settings get' to see all settings.`,
				);
			}

			const value = lookupByDottedKey(settings, key);
			output.result({ key, value }, () => {
				process.stdout.write(formatValue(value) + '\n');
			});
		}));
}

// ---------------------------------------------------------------------------
// app settings set
// ---------------------------------------------------------------------------

function registerSet(settingsCmd: Command): void {
	const arrayKeys = Object.keys(SCHEMA).filter((k) => SCHEMA[k] === 'string-array');
	settingsCmd
		.command('set')
		.argument('<key>', 'Setting key (dotted for nested)')
		.argument(
			'<value>',
			`New value. List settings (${arrayKeys.join(', ')}) take a comma-separated value; ` +
				'pass an empty string ("") to clear the list.',
		)
		.option('--confirm-app-restart', 'Confirm that the CLI may ask Windhawk to restart if the setting demands it.')
		.description('Set a Windhawk application setting.')
		.action((key: string, rawValue: string, cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			const type = SCHEMA[key];
			if (!type) {
				throw new UsageError(
					`Unknown app setting '${key}'. Run 'app settings get' to see all settings.`,
				);
			}

			const newValue = parseValue(key, type, rawValue);
			const patch = buildPatch(key, newValue);

			const beforeSettings = await env.core.getAppSettings();
			const previousValue = lookupByDottedKey(beforeSettings, key);

			// A setting that reads as null is in the schema but not applicable in
			// this installation mode (e.g. disableRunUIScheduledTask in portable
			// mode). Reject it here as a usage error (exit 2) instead of letting
			// the core throw a plain Error that maps to GENERIC (exit 1).
			if (previousValue === null) {
				throw new UsageError(
					`Setting '${key}' is not available in this Windhawk installation mode.`,
				);
			}

			// The refusal gate needs the restart intent BEFORE anything is
			// written; applyAppSettings only reports intents after writing, so
			// use the pure preview command.
			const preview = await env.core.previewAppSettingsEffects(patch);
			if (preview.requiresRestart && !cmdOpts.confirmAppRestart) {
				throw new RestartRequiredError(
					`Setting '${key}' requires a Windhawk restart. Pass --confirm-app-restart to proceed.`,
				);
			}

			const { requiresRestart, requiresNotify } = await env.core.applyAppSettings(patch);

			// Tray notification: matches the extension's updateAppSettings
			// handler exactly. This is the one CLI command that spawns the tray
			// program.
			if (requiresRestart) {
				await env.core.notifyTray('restartBg');
			} else if (requiresNotify) {
				await env.core.notifyTray('appSettingsChanged');
			}

			output.result(
				{
					key,
					value: newValue,
					previousValue,
					restartRequested: requiresRestart,
					notifyRequested: requiresNotify,
				},
				() => {
					process.stdout.write(
						`${key}: ${formatValue(previousValue)} -> ${formatValue(newValue)}\n`,
					);
					if (requiresRestart) {
						process.stdout.write('Windhawk restart requested.\n');
					} else if (requiresNotify) {
						process.stdout.write('Tray notified; engine will pick up the change.\n');
					}
				},
			);
		}));
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

function lookupByDottedKey(settings: AppSettings, key: string): unknown {
	const parts = key.split('.');
	let cursor: unknown = settings;
	for (const part of parts) {
		if (cursor === null || typeof cursor !== 'object') {
			return undefined;
		}
		cursor = (cursor as Record<string, unknown>)[part];
	}
	return cursor;
}

function buildPatch(key: string, value: string | number | boolean | string[]): Partial<AppSettings> {
	// Only engine.<x> has nesting. Everything else is top-level.
	if (key.startsWith('engine.')) {
		const engineKey = key.slice('engine.'.length);
		return { engine: { [engineKey]: value } as AppSettings['engine'] };
	}
	return { [key]: value } as Partial<AppSettings>;
}

function parseValue(key: string, type: FieldType, raw: string): string | number | boolean | string[] {
	if (type === 'string') {
		return raw;
	}
	if (type === 'boolean') {
		if (raw === 'true' || raw === '1') {
			return true;
		}
		if (raw === 'false' || raw === '0') {
			return false;
		}
		throw new UsageError(
			`Setting '${key}' is boolean; value must be one of true/false/1/0, got '${raw}'.`,
		);
	}
	if (type === 'number') {
		return parseInt32Setting(key, raw);
	}
	// string-array: comma-separated, surrounding whitespace trimmed per item so
	// the `a, b` form printed by `get` round-trips. Empty string -> empty array.
	return raw === '' ? [] : raw.split(',').map((item) => item.trim());
}

function formatFlatSettings(settings: AppSettings): string[] {
	const lines: string[] = [];
	// Sort SCHEMA keys so output is deterministic and complete.
	for (const key of Object.keys(SCHEMA).sort()) {
		const value = lookupByDottedKey(settings, key);
		lines.push(`${key}=${formatValue(value)}`);
	}
	return lines;
}

function formatValue(value: unknown): string {
	if (value === null) {
		return '<null>';
	}
	if (value === undefined) {
		return '<unset>';
	}
	if (Array.isArray(value)) {
		return value.length === 0 ? '<empty list>' : value.join(', ');
	}
	return String(value);
}
