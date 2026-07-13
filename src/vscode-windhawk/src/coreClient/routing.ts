import { isDeepStrictEqual } from 'util';
import { Logger } from '../services/logger';
import { WindhawkCore } from './contract';
import type { DllBackend } from './dllBackend';

// The per-command routing table of the native core migration: each
// contract command is served by windhawk-core.dll when flagged here, by the
// in-process TypeScript backend otherwise. Routing flips whole commands, never
// halves of a flow.

export type CommandName = keyof WindhawkCore;

// Every contract command, mirroring the WindhawkCore interface (a unit
// test asserts this list matches the interface).
export const ALL_COMMANDS: readonly CommandName[] = [
	'getCoreInfo',
	'parseModSource',
	'appendToModIdAndName',
	'listInstalledMods',
	'getModSource',
	'doesModExist',
	'getModConfig',
	'updateModConfig',
	'getModSettings',
	'setModSettings',
	'setModLoggingEnabled',
	'setModRating',
	'installMod',
	'compileInstalledMod',
	'setModEnabled',
	'removeMod',
	'applyAppSettings',
	'previewAppSettingsEffects',
	'syncCatalogToProfile',
	'getAppSettings',
	'fetchCatalog',
	'fetchRepoModSource',
	'fetchModVersions',
	'getAppUpdateStatus',
	'getProfileWatchInfo',
	'notifyTray',
	'startUpdate',
	'getCompileFlags',
];

// Commands served by the DLL when its artifacts are present - the full
// command inventory, so this set equals ALL_COMMANDS (a routing test asserts
// the equality). The in-process backend stays the per-command fallback (when
// the DLL is missing or a command is routed back) and the dual-run reference.
export const DEFAULT_DLL_COMMANDS: ReadonlySet<CommandName> = new Set<CommandName>([
	'parseModSource',
	'appendToModIdAndName',
	'getCoreInfo',
	'getAppSettings',
	'applyAppSettings',
	'previewAppSettingsEffects',
	'getModConfig',
	'updateModConfig',
	'getModSettings',
	'setModSettings',
	'setModLoggingEnabled',
	'listInstalledMods',
	'getModSource',
	'doesModExist',
	'setModRating',
	'installMod',
	'setModEnabled',
	'removeMod',
	'syncCatalogToProfile',
	'getAppUpdateStatus',
	'getProfileWatchInfo',
	'fetchCatalog',
	'fetchRepoModSource',
	'fetchModVersions',
	'startUpdate',
	'compileInstalledMod',
	'notifyTray',
	'getCompileFlags',
]);

// Commands safe to execute on BOTH backends for the dual-run diff mode:
// no writes, no network, and not parameter-dependent (listInstalledMods
// can write through syncProfile, so it stays out). getProfileWatchInfo is
// excluded once it is DLL-routed: its lastModifiedByUserMtimeMs is
// session-local bookkeeping (each backend tracks only its own writes), so a
// cross-backend diff is expected, not a discrepancy worth logging.
export const DUAL_RUN_SAFE_COMMANDS: ReadonlySet<CommandName> = new Set<CommandName>([
	'getCoreInfo',
	'parseModSource',
	'appendToModIdAndName',
	'getModSource',
	'doesModExist',
	'getModConfig',
	'getModSettings',
	'getAppSettings',
	'previewAppSettingsEffects',
	'getAppUpdateStatus',
	// A pure, no-param, deterministic read: both backends return the same
	// fixed flag set, so a cross-backend diff is meaningful.
	'getCompileFlags',
]);

export type RoutingConfig = {
	dllCommands: ReadonlySet<CommandName>;
	// Debug builds only: run dual-run-safe DLL-routed commands on both
	// backends and log result diffs.
	dualRun: boolean;
};

// Development overrides, gated like the other WINDHAWK_DEBUG_* vars (the
// production build drops the branch):
//   WINDHAWK_DEBUG_CORE_COMMANDS - 'none', 'default', or a comma list of
//     command names to route to the DLL.
//   WINDHAWK_DEBUG_CORE_DUAL_RUN - '1' enables the dual-run diff mode.
export function resolveRoutingFromEnv(): RoutingConfig {
	let dllCommands: ReadonlySet<CommandName> = DEFAULT_DLL_COMMANDS;
	let dualRun = false;
	if (process.env.NODE_ENV !== 'production') {
		const commands = process.env.WINDHAWK_DEBUG_CORE_COMMANDS;
		if (commands && commands !== 'default') {
			dllCommands = new Set(
				commands === 'none'
					? []
					: commands
						.split(',')
						.map(name => name.trim())
						.filter((name): name is CommandName =>
							(ALL_COMMANDS as readonly string[]).includes(name),
						),
			);
		}
		dualRun = process.env.WINDHAWK_DEBUG_CORE_DUAL_RUN === '1';
	}
	return { dllCommands, dualRun };
}

function normalizeForDiff(value: unknown): unknown {
	// Round-trip through JSON so both backends are compared at the wire
	// altitude (drops undefined properties, collapses 1.0 vs 1, etc.).
	return value === undefined ? undefined : JSON.parse(JSON.stringify(value));
}

function describeOutcome(outcome: PromiseSettledResult<unknown>): string {
	const text =
		outcome.status === 'fulfilled'
			? JSON.stringify(normalizeForDiff(outcome.value))
			: `rejected: ${String(outcome.reason)}`;
	const max = 2000;
	return text !== undefined && text.length > max ? `${text.slice(0, max)}...` : String(text);
}

// Rejection code pairs that differ between the backends BY DESIGN, so dual-run
// must not flag them: the in-process backend rejects a missing mod source with
// the raw Node ENOENT, while the DLL maps it to MOD_NOT_INSTALLED (for
// getModSource). Keyed as the sorted "a|b" pair. Any code difference
// NOT listed here is a real divergence to surface.
const EQUIVALENT_REJECTION_CODES: ReadonlySet<string> = new Set<string>([
	['ENOENT', 'MOD_NOT_INSTALLED'].sort().join('|'),
]);

// The stable error code carried by a rejection reason, if any: the `.code` of a
// WindhawkError / CoreDllError (the DLL backend) or a Node ErrnoException (the
// in-process backend). undefined for a plain Error with no code.
function rejectionCode(reason: unknown): string | undefined {
	const code = (reason as { code?: unknown } | null | undefined)?.code;
	return typeof code === 'string' ? code : undefined;
}

function rejectionsMatch(dllReason: unknown, tsReason: unknown): boolean {
	const dllCode = rejectionCode(dllReason);
	const tsCode = rejectionCode(tsReason);
	if (dllCode === tsCode) {
		return true;
	}
	return EQUIVALENT_REJECTION_CODES.has([dllCode ?? '', tsCode ?? ''].sort().join('|'));
}

function outcomesMatch(
	dll: PromiseSettledResult<unknown>,
	ts: PromiseSettledResult<unknown>,
): boolean {
	if (dll.status !== ts.status) {
		return false;
	}
	if (dll.status === 'fulfilled' && ts.status === 'fulfilled') {
		return isDeepStrictEqual(normalizeForDiff(dll.value), normalizeForDiff(ts.value));
	}
	// Both rejected: the human wording may still differ across backends, but the
	// stable error code must match. A DLL failure classifying to a different code
	// (and thus, in the CLI, a different exit code) than the in-process reference
	// is exactly the divergence dual-run exists to catch - the both-rejected
	// branch used to return `true` unconditionally, which is how the
	// CoreDllError exit-code regression went unnoticed.
	return rejectionsMatch(
		(dll as PromiseRejectedResult).reason,
		(ts as PromiseRejectedResult).reason,
	);
}

type AnyAsyncFn = (...args: unknown[]) => Promise<unknown>;

function withDualRun(
	name: CommandName,
	dllFn: AnyAsyncFn,
	tsFn: AnyAsyncFn,
	logger: Logger,
): AnyAsyncFn {
	return async (...args: unknown[]) => {
		const [dllOutcome, tsOutcome] = await Promise.allSettled([
			dllFn(...args),
			tsFn(...args),
		]);
		if (!outcomesMatch(dllOutcome, tsOutcome)) {
			logger.warn(
				`windhawk-core dual-run diff for ${name}:\n` +
				`  dll: ${describeOutcome(dllOutcome)}\n` +
				`  ts:  ${describeOutcome(tsOutcome)}`,
			);
		}
		// The DLL is the routed backend; its outcome is the command's.
		if (dllOutcome.status === 'fulfilled') {
			return dllOutcome.value;
		}
		throw dllOutcome.reason;
	};
}

// Compose the routed core: DLL-flagged commands the DLL backend implements
// go native (optionally dual-run); everything else stays on the in-process
// TypeScript backend.
export function buildRoutedCore(
	tsBackend: WindhawkCore,
	dllCommands: Partial<WindhawkCore>,
	routing: RoutingConfig,
	logger: Logger,
): WindhawkCore {
	const routed: Record<string, unknown> = {};
	for (const name of ALL_COMMANDS) {
		const tsFn = (tsBackend[name] as (...args: unknown[]) => unknown).bind(tsBackend);
		const dllFn = (dllCommands[name] as AnyAsyncFn | undefined)?.bind(dllCommands);
		if (dllFn && routing.dllCommands.has(name)) {
			routed[name] =
				routing.dualRun && DUAL_RUN_SAFE_COMMANDS.has(name)
					? withDualRun(name, dllFn, tsFn as AnyAsyncFn, logger)
					: dllFn;
		} else {
			routed[name] = tsFn;
		}
	}
	return routed as unknown as WindhawkCore;
}

// Choose and build the routed core: when the routing table flags any command
// for the DLL, load it (via `makeDllBackend`) and route the flagged commands
// there; otherwise - or if the DLL fails to load - serve everything in-process.
// A missing or unloadable windhawk-core.dll never fails creation: the factory's
// throw is caught and logged at info, and the in-process backend serves every
// command. Extracted from createWindhawkCore so this
// selection - especially the fallback - is unit-testable without the native
// storage/bridge load chain createWindhawkCore otherwise pulls in.
export function selectCore(
	inProcess: WindhawkCore,
	makeDllBackend: () => DllBackend,
	routing: RoutingConfig,
	logger: Logger,
): WindhawkCore {
	if (routing.dllCommands.size === 0) {
		return inProcess;
	}
	try {
		const dll = makeDllBackend();
		return buildRoutedCore(inProcess, dll.commands, routing, logger);
	} catch (e) {
		logger.info(
			`windhawk-core DLL backend unavailable, serving all commands in-process (${e instanceof Error ? e.message : String(e)})`,
		);
		return inProcess;
	}
}
