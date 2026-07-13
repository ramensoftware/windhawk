import { AsyncOperation, WindhawkError } from '../coreClient/contract';
// Type-only: importing environment.ts for its value (loadEnvironment) would pull
// the whole core-client/service/native-module chain into anything that imports
// runCommand. loadEnvironment is loaded lazily (or injected) inside runCommand
// instead, so this module - and its SIGINT handler - stay unit-testable.
import type { Environment, GlobalOpts } from './environment';
import { createOutput, Output } from './output';

// Per-action wrapper. Loads the environment, wires the output adapter, runs
// the action, and maps thrown errors to the right exit code. Every CLI
// command action goes through here.

export type CommandContext = {
	env: Environment;
	output: Output;
	// Run a cancelable core operation: registers it so the SIGINT handler can
	// cancel it, awaits its result, and unregisters it.
	track<T>(op: AsyncOperation<T>): Promise<T>;
};

export type CommandAction = (ctx: CommandContext) => Promise<void>;

// The environment loader. Defaults to the real loadEnvironment (lazily imported
// to keep this module light); tests inject a fake to avoid the native chain.
export type EnvLoader = (opts: GlobalOpts) => Environment;

// Tracked across the process so the SIGINT handler can reach into whatever
// action is currently running. Set once per command invocation.
let currentContext: {
	output: Output;
	activeOperations: Set<AsyncOperation<unknown>>;
} | null = null;

export async function runCommand(
	globalOpts: GlobalOpts,
	action: CommandAction,
	loadEnv?: EnvLoader,
): Promise<void> {
	const output = createOutput(globalOpts.json);
	try {
		const load = loadEnv ?? (await import(/* webpackMode: "eager" */ './environment')).loadEnvironment;
		const env = load(globalOpts);
		const activeOperations = new Set<AsyncOperation<unknown>>();
		const ctx: CommandContext = {
			env,
			output,
			async track(op) {
				activeOperations.add(op);
				try {
					return await op.result;
				} finally {
					activeOperations.delete(op);
				}
			},
		};
		currentContext = { output, activeOperations };
		// Keep the event loop alive for the duration of the command. The
		// windhawk-core bridge unrefs the threadsafe callbacks that deliver
		// its command results to the JS thread (so a leaked session cannot
		// keep the process alive - see src/coreClient/dllBackend.ts). In the
		// VSCode extension the Electron host keeps the loop alive regardless,
		// but in the standalone CLI nothing else holds it open: without this
		// ref'd handle Node would drain the loop and exit 0 right after the
		// session is created, before the first DLL command (getAppSettings,
		// fetchCatalog, ...) ever resolves - so stdout stays empty. A ref'd
		// timer keeps libuv running; the unref'd callbacks still fire while it
		// is alive, and clearing it lets the process exit promptly when done.
		const keepAlive = setInterval(() => {}, 1 << 30);
		try {
			await action(ctx);
		} finally {
			clearInterval(keepAlive);
			currentContext = null;
		}
	} catch (e) {
		output.error(e);
	}
}

// SIGINT (Ctrl+C) handling, exported for testing. Cancels any in-flight tracked
// operation (compile child processes are killed, update downloads aborted -
// cancel() is synchronous), then exits with the CANCELLED code via the same
// output adapter as other errors so --json callers still get a structured error
// envelope. When no command is active (the signal arrived during commander's
// parse phase or similar), output.error is skipped and we just exit 9.
export function handleSigint(): void {
	if (currentContext) {
		const { output, activeOperations } = currentContext;
		for (const op of activeOperations) {
			try {
				op.cancel();
			} catch {
				// Swallow: we're exiting anyway.
			}
		}
		// output.error never returns (it calls process.exit with the CANCELLED
		// exit code, 9); the explicit exit below covers the no-context case.
		output.error(new WindhawkError('CANCELLED', 'Cancelled by user'));
	}
	process.exit(9);
}

export function installSignalHandlers(): void {
	process.on('SIGINT', handleSigint);
}
