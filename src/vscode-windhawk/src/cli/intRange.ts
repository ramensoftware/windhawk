import { UsageError } from './errors';

// Numeric settings are stored as 32-bit values - REG_DWORD in non-portable
// mode - and read by the engine as a 32-bit int. The signed 32-bit range is
// the only one that round-trips correctly under both interpretations
// (unsigned [2^31, 2^32-1] values would read back as negative ints), so values
// outside it must be rejected rather than silently truncated on write.
export const INT32_MIN = -2147483648;
export const INT32_MAX = 2147483647;

// Parse a setting string into a validated 32-bit integer. Rejects floats,
// non-numeric input, and out-of-range values with a UsageError (exit 2).
export function parseInt32Setting(key: string, raw: string): number {
	if (!/^-?\d+$/.test(raw)) {
		throw new UsageError(`Setting '${key}' must be an integer, got '${raw}'.`);
	}
	const n = Number(raw);
	if (n < INT32_MIN || n > INT32_MAX) {
		throw new UsageError(
			`Setting '${key}' must be a 32-bit integer (${INT32_MIN}..${INT32_MAX}), got '${raw}'.`,
		);
	}
	return n;
}
