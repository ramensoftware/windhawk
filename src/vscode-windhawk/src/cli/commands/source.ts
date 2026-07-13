import type { Command } from 'commander';
import * as fs from 'fs';
import { UsageError } from '../errors';
import { withOutput } from '../withCommand';

export function registerSourceCommands(program: Command): void {
	const sourceCmd = program.command('source').description('Operate on .wh.cpp mod source files directly.');

	registerMeta(sourceCmd);
}

function registerMeta(sourceCmd: Command): void {
	sourceCmd
		.command('meta')
		.argument('<file>', 'Path to a .wh.cpp mod source file')
		.description('Extract metadata from a .wh.cpp file.')
		.action((file: string, _cmdOpts, cmd) => withOutput(cmd, async (output) => {
			let source: string;
			try {
				source = fs.readFileSync(file, 'utf8');
			} catch (e) {
				const message = e instanceof Error ? e.message : String(e);
				throw new UsageError(`Failed to read '${file}': ${message}`);
			}

			// parseModSource is a pure helper; the session-free variant keeps
			// `source meta` independent of environment discovery, which it
			// explicitly doesn't need. Deferred import so the --help fast path
			// doesn't load the parser's dependency chain.
			const { parseModSourceStandalone } = await import(/* webpackMode: "eager" */ '../../coreClient/parseModSource');
			const parsed = parseModSourceStandalone(source, 'en');
			if (!parsed.metadata) {
				const message = parsed.errors.metadata ?? 'Failed to parse mod metadata';
				throw new UsageError(`Failed to parse metadata from '${file}': ${message}`);
			}
			const metadata = parsed.metadata as Record<string, unknown>;

			output.result({ metadata }, () => {
				for (const [key, value] of Object.entries(metadata)) {
					const rendered = Array.isArray(value) ? value.join(', ') : String(value);
					process.stdout.write(`${key}: ${rendered}\n`);
				}
			});
		}));
}
