import ModSource from '../services/modSource';
import { ParsedModSource } from './contract';

// Shared implementation of the contract's parseModSource command: parse the
// three sections independently so one malformed block doesn't hide the
// others. Used by the in-process backend (against its ModSource instance)
// and by parseModSourceStandalone below.
export function parseModSourceWith(
	modSource: ModSource,
	source: string,
	language: string,
): ParsedModSource {
	const result: ParsedModSource = {
		metadata: null,
		readme: null,
		initialSettings: null,
		errors: {},
	};

	try {
		result.metadata = modSource.extractMetadata(source, language);
	} catch (e) {
		result.errors.metadata = e instanceof Error ? e.message : String(e);
	}

	try {
		result.readme = modSource.extractReadme(source);
	} catch (e) {
		result.errors.readme = e instanceof Error ? e.message : String(e);
	}

	try {
		result.initialSettings = modSource.extractInitialSettings(source, language);
	} catch (e) {
		result.errors.initialSettings = e instanceof Error ? e.message : String(e);
	}

	return result;
}

// Session-free variant for callers that have no Windhawk environment at all
// (the CLI's `source meta`, which works without an app root). parseModSource is
// a pure helper, so this is the same code path the session command takes; the
// ModSource extract methods use no instance state, making an empty appDataPath
// safe.
export function parseModSourceStandalone(source: string, language: string): ParsedModSource {
	return parseModSourceWith(new ModSource(''), source, language);
}
