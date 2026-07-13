// Minimal logger contract for user-facing messages, decoupled from any
// specific UI host. Services that need to surface information to the user
// (e.g. a failed tray-program spawn) take a Logger in their constructor
// rather than writing to a specific sink. Callers provide whatever
// implementation makes sense in their environment.

export interface Logger {
	error(msg: string): void;
	warn(msg: string): void;
	info(msg: string): void;
}

export const NoopLogger: Logger = {
	error() { /* no-op */ },
	warn() { /* no-op */ },
	info() { /* no-op */ },
};
