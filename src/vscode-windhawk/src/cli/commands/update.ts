import type { Command } from 'commander';
import { UsageError } from '../errors';
import { getInstalledWindhawkVersion } from '../windhawkVersion';
import { withCommand } from '../withCommand';

export function registerUpdateCommands(program: Command): void {
	const updateCmd = program.command('update').description('Query and install Windhawk updates.');

	registerStatus(updateCmd);
	registerRun(updateCmd);
}

// ---------------------------------------------------------------------------
// update status
// ---------------------------------------------------------------------------

function registerStatus(updateCmd: Command): void {
	updateCmd
		.command('status')
		.description('Show the cached latest Windhawk version and compare with the installed version.')
		.action((_cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			// Installed version comes from the CLI's bundled package.json, the
			// same source the extension uses (and the same value the core
			// session was created with).
			const installedVersion = getInstalledWindhawkVersion()?.version ?? null;

			// The bleeding-edge value is the raw cached latest version;
			// latestVersion is the grace-period-filtered value used for the
			// GUI's update badge and has no place in an on-demand CLI read.
			const status = await env.core.getAppUpdateStatus();
			const latestVersion = status.latestVersionBleedingEdge;
			const updateAvailable = status.updateAvailableBleedingEdge;

			output.result(
				{
					installedVersion,
					latestVersion,
					updateAvailable,
				},
				() => {
					process.stdout.write(`Installed:        ${installedVersion ?? 'unknown'}\n`);
					process.stdout.write(`Latest:           ${latestVersion ?? 'unknown'}\n`);
					process.stdout.write(`Update available: ${updateAvailable ? 'yes' : 'no'}\n`);
				},
			);
		}));
}

// ---------------------------------------------------------------------------
// update run
// ---------------------------------------------------------------------------

function registerRun(updateCmd: Command): void {
	updateCmd
		.command('run')
		.description('Download and launch the Windhawk installer. Requires --yes.')
		.action((_cmdOpts, cmd) => withCommand(cmd, async (ctx) => {
			const { env, output } = ctx;
			if (!env.globalOpts.yes) {
				// --yes is required for `update run`. Without it, print the
				// planned action and exit 2. Same pattern as `mod remove`.
				process.stderr.write(
					'Would download and launch the Windhawk installer. Pass --yes to confirm.\n',
				);
				throw new UsageError('Refusing to run the installer without --yes');
			}

			let lastReportedProgress = -1;
			// Tracked so Ctrl+C cancels the download via the operation handle.
			await ctx.track(env.core.startUpdate({
				onProgress: ({ progress }) => {
					// The core already de-duplicates identical progress values;
					// this guard is a safety net for --quiet plus future API
					// changes. Progress lives on stderr in both text and JSON
					// modes so stdout stays clean for the single completion
					// object.
					if (env.globalOpts.quiet) {
						return;
					}
					if (progress !== lastReportedProgress) {
						lastReportedProgress = progress;
						process.stderr.write(`Downloading: ${progress}%\n`);
					}
				},
				onInstalling: () => {
					if (!env.globalOpts.quiet) {
						process.stderr.write('Launching installer...\n');
					}
				},
			}));

			// Any thrown error (RepoUnreachableError, Ctrl+C AbortError,
			// installer-spawn failure) propagates to the output adapter and
			// maps to its usual exit code (6 / 9 / 1 respectively).

			// The update flow doesn't return the version it just pulled; cite
			// the user-profile cache as the best-effort source.
			const status = await env.core.getAppUpdateStatus();
			const version = status.latestVersionBleedingEdge || '';

			output.result(
				{ version, installerLaunched: true },
				() => {
					process.stdout.write(
						`Installer launched${version ? `: ${version}` : ''}\n`,
					);
				},
			);
		}));
}
