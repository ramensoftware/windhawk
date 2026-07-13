import type { Command } from 'commander';
// Type-only imports below: ensures loading this module does not pull in the
// environment/service/native-module chain. --help and --version fast paths
// stay free of fs-ext / native-reg loads.
import type { GlobalOpts } from './environment';
import type { Output } from './output';
import type { CommandAction } from './runCommand';

// Wrapper used by commands that need the full Windhawk environment (services,
// logger, storage paths). Collects global options from the command's
// inherited-option scope, then dynamically imports runCommand so the heavy
// service bootstrap only loads when a command actually runs.
export async function withCommand(cmd: Command, action: CommandAction): Promise<void> {
	const globalOpts = collectGlobalOpts(cmd);
	const { runCommand } = await import(/* webpackMode: "eager" */ './runCommand');
	await runCommand(globalOpts, action);
}

// Wrapper for commands that only need output formatting, no environment or
// services (e.g. `source meta <file>` which parses a file argument directly).
// Cheaper: skips app-root resolution and the native-module load chain.
export async function withOutput(cmd: Command, action: (output: Output) => Promise<void>): Promise<void> {
	const globalOpts = collectGlobalOpts(cmd);
	const { createOutput } = await import(/* webpackMode: "eager" */ './output');
	const output = createOutput(globalOpts.json);
	try {
		await action(output);
	} catch (e) {
		output.error(e);
	}
}

function collectGlobalOpts(cmd: Command): GlobalOpts {
	const merged = cmd.optsWithGlobals();
	return {
		appRoot: merged.appRoot,
		json: !!merged.json,
		yes: !!merged.yes,
		quiet: !!merged.quiet,
	};
}
