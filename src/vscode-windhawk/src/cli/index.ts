#!/usr/bin/env node
import { Command, CommanderError } from 'commander';
import { registerAppCommands } from './commands/app';
import { registerModCommands } from './commands/mod';
import { registerRepoCommands } from './commands/repo';
import { registerSourceCommands } from './commands/source';
import { registerUpdateCommands } from './commands/update';
import { UsageError } from './errors';
import { createOutput } from './output';
import { readCliVersion } from './windhawkVersion';

const program = new Command();

// Make commander throw CommanderError instead of calling process.exit() on
// parse errors, so commander-level usage errors (missing arg, unknown command
// or option, bad value) get the same treatment as action-level ones: exit code
// 2 (USAGE) and, in --json mode, a structured envelope. Must be set before the
// subcommands are registered - copyInheritedSettings propagates it to each
// subcommand at creation time.
program.exitOverride();

program
	.name('windhawk-cli')
	.description('Command-line interface for Windhawk')
	.version(readCliVersion(), '-v, --version', 'Print the CLI version and exit.')
	.option('--app-root <path>', 'Override Windhawk app root (directory containing windhawk.ini).')
	.option('--json', 'Emit JSON output on stdout instead of human-readable text.')
	.option('--yes', 'Skip confirmation for destructive operations.')
	.option('--quiet', 'Suppress non-essential stderr output (errors and warnings still print).');

registerAppCommands(program);
registerModCommands(program);
registerRepoCommands(program);
registerSourceCommands(program);
registerUpdateCommands(program);

// SIGINT handler is wired via dynamic import so --help / --version do not
// transitively load the native-module chain. webpackMode: "eager" inlines
// the module into the bundle (no extra chunk file) while keeping
// evaluation deferred until this import() is actually awaited.
void (async () => {
	const { installSignalHandlers } = await import(/* webpackMode: "eager" */ './runCommand');
	installSignalHandlers();
})();

program.parseAsync(process.argv).catch((e: unknown) => {
	if (e instanceof CommanderError) {
		// --help / --version: commander already wrote to stdout and uses a zero
		// exit code. Not an error.
		if (e.exitCode === 0) {
			process.exit(0);
		}
		// Parse/usage error. Commander already wrote "error: <msg>" to stderr.
		// Map to the spec's USAGE exit code (2); in --json mode also emit the
		// structured envelope on stdout. createOutput writes only the envelope
		// in json mode (and exits 2), so stderr keeps commander's human message.
		const json = !!program.opts().json || process.argv.includes('--json');
		if (json) {
			// Commander bakes an "error: " prefix into the message; strip it so
			// the envelope carries a clean message.
			createOutput(true).error(new UsageError(e.message.replace(/^error: /, '')));
		}
		process.exit(2);
	}
	// Any other rejection is genuinely unexpected.
	const msg = e instanceof Error ? e.message : String(e);
	process.stderr.write(`error: ${msg}\n`);
	process.exit(1);
});
