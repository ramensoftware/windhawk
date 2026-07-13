import type { Command } from 'commander';
import type { ModConfig, ModMetadata, RepositoryDetails } from '../../coreClient/contract';
import { withCommand } from '../withCommand';

export function registerRepoCommands(program: Command): void {
	const repoCmd = program.command('repo').description('Query the Windhawk mod repository.');

	registerRepoList(repoCmd);
	registerRepoVersions(repoCmd);
	registerRepoShow(repoCmd);
}

// ---------------------------------------------------------------------------
// repo list
// ---------------------------------------------------------------------------

type RepoListRow = {
	id: string;
	metadata: ModMetadata;
	details: RepositoryDetails;
	installed?: {
		metadata: ModMetadata | null;
		config: ModConfig | null;
		userRating: number;
	};
};

function registerRepoList(repoCmd: Command): void {
	repoCmd
		.command('list')
		.description('List all mods in the repository.')
		.option('--with-installed', 'Also include installed-state data per mod.')
		.action((cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			const appSettings = await env.core.getAppSettings();
			const language = appSettings.language || 'en';
			const catalog = await env.core.fetchCatalog(language);

			// Record the catalog's latest versions in the user profile so it
			// stays in sync across GUI/CLI access. Unlike the GUI, the CLI does
			// NOT post the tray "new updates found" notification: only `app
			// settings set` spawns windhawk.exe; the engine/GUI picks up the
			// new-update state from the written profile on its own.
			await env.core.syncCatalogToProfile(catalog);

			const rows: RepoListRow[] = [];
			let installedMods: Record<string, {
				metadata: ModMetadata | null;
				config: ModConfig | null;
				userRating: number;
			}> = {};
			if (cmdOpts.withInstalled) {
				// Pure read: decorating the repo listing with installed state
				// must not write the profile (the sync above already did any
				// needed writing).
				const { mods, loadErrors } = await env.core.listInstalledMods({
					language,
					checkForUpdates: !appSettings.disableUpdateCheck,
					syncProfile: false,
				});
				for (const { modId, error } of loadErrors) {
					env.logger.warn(`Failed to load metadata for mod '${modId}': ${error}`);
				}
				installedMods = mods;
			}

			for (const id of Object.keys(catalog.mods).sort((a, b) => a.localeCompare(b))) {
				const entry = catalog.mods[id];
				const row: RepoListRow = {
					id,
					metadata: entry.metadata,
					details: entry.details,
				};
				const installed = installedMods[id];
				if (cmdOpts.withInstalled && installed && (installed.metadata || installed.config)) {
					row.installed = {
						metadata: installed.metadata,
						config: installed.config,
						userRating: installed.userRating,
					};
				}
				rows.push(row);
			}

			output.result({ mods: rows }, () => {
				for (const row of rows) {
					const version = row.metadata.version ?? '';
					const name = row.metadata.name ?? '';
					const marker = row.installed ? '\t[installed]' : '';
					process.stdout.write(`${row.id}\t${version}\t${name}${marker}\n`);
				}
			});
		}));
}

// ---------------------------------------------------------------------------
// repo versions
// ---------------------------------------------------------------------------

function registerRepoVersions(repoCmd: Command): void {
	repoCmd
		.command('versions')
		.argument('<id>', 'Mod ID')
		.description('List all published versions of a mod.')
		.action((id: string, _cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			// Shape validation and isPreRelease derivation happen in the core;
			// a non-JSON or unexpected-shape response surfaces as
			// RepoUnreachableError (exit 6).
			const versions = await env.core.fetchModVersions(id);

			output.result({ id, versions }, () => {
				for (const v of versions) {
					const iso = new Date(v.timestamp * 1000).toISOString();
					const mark = v.isPreRelease ? '\t[pre-release]' : '';
					process.stdout.write(`${v.version}\t${iso}${mark}\n`);
				}
			});
		}));
}

// ---------------------------------------------------------------------------
// repo show
// ---------------------------------------------------------------------------

function registerRepoShow(repoCmd: Command): void {
	repoCmd
		.command('show')
		.argument('<id>', 'Mod ID')
		.argument('[version]', 'Specific version to fetch. Default is latest.')
		.description('Show repository metadata, README, and initial settings for a mod.')
		.action((id: string, version: string | undefined, _cmdOpts, cmd) => withCommand(cmd, async ({ env, output }) => {
			// Source arrives CRLF-normalized from the core, the same
			// normalization the extension applies before parsing.
			const source = await env.core.fetchRepoModSource(id, version);

			const language = (await env.core.getAppSettings()).language || 'en';
			const parsed = await env.core.parseModSource(source, language);
			// Parse failures surface as a generic failure (exit 1), matching
			// the previous direct extract* calls on a fetched source.
			if (!parsed.metadata) {
				throw new Error(parsed.errors.metadata ?? 'Failed to parse mod metadata');
			}
			if (parsed.errors.initialSettings !== undefined) {
				throw new Error(parsed.errors.initialSettings);
			}
			const { metadata, readme, initialSettings } = parsed;

			const resolvedVersion = metadata.version ?? version ?? '';
			output.result(
				{ id, version: resolvedVersion, metadata, readme, initialSettings },
				() => {
					process.stdout.write(`ID:            ${id}\n`);
					process.stdout.write(`Name:          ${metadata.name ?? ''}\n`);
					process.stdout.write(`Version:       ${resolvedVersion}\n`);
					process.stdout.write(`Author:        ${metadata.author ?? ''}\n`);
					if (metadata.architecture?.length) {
						process.stdout.write(`Architectures: ${metadata.architecture.join(', ')}\n`);
					}
					if (metadata.description) {
						process.stdout.write('\nDescription:\n');
						for (const line of metadata.description.split('\n')) {
							process.stdout.write(`  ${line}\n`);
						}
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
