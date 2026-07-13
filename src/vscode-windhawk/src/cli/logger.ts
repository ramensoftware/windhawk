import { Logger } from '../coreClient/contract';

// stderr-backed Logger for the core to surface messages.
//
// Routes:
//   error(msg) -> always printed, prefixed with "error: "
//   warn(msg)  -> always printed, prefixed with "warning: "
//   info(msg)  -> printed unless --quiet was passed
//
// All output goes to stderr so that stdout stays clean for the command's
// result.
export function createStderrLogger(opts: { quiet: boolean }): Logger {
	return {
		error(msg) {
			process.stderr.write(`error: ${msg}\n`);
		},
		warn(msg) {
			process.stderr.write(`warning: ${msg}\n`);
		},
		info(msg) {
			if (!opts.quiet) {
				process.stderr.write(`${msg}\n`);
			}
		},
	};
}
