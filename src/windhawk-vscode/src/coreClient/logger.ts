// Minimal logger contract for user-facing messages, decoupled from any
// specific UI host. The core client and the front-ends surface information to
// the user (VSCode notifications, CLI stderr) through this interface rather
// than writing to a specific sink; the host injects its implementation when
// creating the core.

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
