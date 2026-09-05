// The mod source's include/define preamble, lifted into a precompiled-header
// source. The core precompiles the result per compile target and feeds it to the
// compile with -include-pch, so what it holds has to be what the mod source
// itself parses first: windhawk_api.h, then the mod's own #include, #define and
// #undef directives.

// Only directives outside comments count, so the metadata, readme, and settings
// blocks a mod source starts with contribute nothing.
export function generatePchHeader(modSource: string) {
	const lines = ['#pragma once', '', '#include <windhawk_api.h>'];

	const directives = extractDirectives(modSource);
	if (directives.length > 0) {
		lines.push('', ...directives);
	}

	return lines.join('\n') + '\n';
}

const directiveRegex = /^\s*#\s*([A-Za-z_]+)/;

const copiedDirectives = ['include', 'define', 'undef'];
const conditionalOpeners = ['if', 'ifdef', 'ifndef'];
const conditionalBranches = ['elif', 'elifdef', 'elifndef', 'else'];

// An #if..#endif range of the mod source, held aside until its #endif tells
// whether anything inside it is worth copying.
type ConditionalGroup = {
	lines: string[];
	hasContent: boolean;
};

// The copied directives, each still wrapped in the conditionals the mod source
// wraps it in, so a per-architecture #define lands in the header under the same
// condition. A conditional with nothing copied inside it is dropped whole.
function extractDirectives(modSource: string) {
	const root: ConditionalGroup = { lines: [], hasContent: true };
	const openGroups = [root];
	const currentGroup = () => openGroups[openGroups.length - 1];

	// Close a group into its parent, keeping it only if something was copied
	// into it.
	const closeGroup = (endLine: string) => {
		const group = openGroups.pop()!;
		const parent = currentGroup();
		if (group.hasContent) {
			parent.lines.push(...group.lines, endLine);
			parent.hasContent = true;
		}
	};

	let inBlockComment = false;
	let inContinuation = false;

	for (const line of modSource.split(/\r?\n/)) {
		const startedInBlockComment = inBlockComment;
		const code = stripComments(line, startedInBlockComment);
		inBlockComment = code.inBlockComment;

		// A directive that opens a block comment contributes only its code, so
		// the comment doesn't reach the header unterminated.
		const copiedLine = (code.inBlockComment ? code.text : line).trimEnd();
		const continues = code.text.trimEnd().endsWith('\\');

		if (inContinuation) {
			currentGroup().lines.push(copiedLine);
			inContinuation = continues;
			continue;
		}

		const directive = startedInBlockComment ? null : directiveRegex.exec(code.text)?.[1];
		if (!directive) {
			continue;
		}

		if (copiedDirectives.includes(directive)) {
			currentGroup().lines.push(copiedLine);
			currentGroup().hasContent = true;
			inContinuation = continues;
		} else if (conditionalOpeners.includes(directive)) {
			openGroups.push({ lines: [copiedLine], hasContent: false });
			inContinuation = continues;
		} else if (conditionalBranches.includes(directive) && openGroups.length > 1) {
			currentGroup().lines.push(copiedLine);
			inContinuation = continues;
		} else if (directive === 'endif' && openGroups.length > 1) {
			closeGroup(copiedLine);
		}
	}

	// A conditional the source never closes (only reachable through a comment
	// the scan read differently than the compiler does) still has to come out as
	// a header that compiles.
	while (openGroups.length > 1) {
		closeGroup('#endif');
	}

	return root.lines;
}

// The line with its comments removed, plus the block-comment state it leaves
// behind for the next line. String and character literals are passed through
// whole, so a `/*` inside one doesn't hide the rest of the file.
function stripComments(line: string, inBlockComment: boolean) {
	let text = '';
	let i = 0;

	while (i < line.length) {
		if (inBlockComment) {
			if (line.startsWith('*/', i)) {
				inBlockComment = false;
				text += ' ';
				i += 2;
			} else {
				i++;
			}
			continue;
		}

		if (line.startsWith('//', i)) {
			break;
		}

		if (line.startsWith('/*', i)) {
			inBlockComment = true;
			i += 2;
			continue;
		}

		if (line[i] === '"' || line[i] === '\'') {
			const end = skipLiteral(line, i);
			text += line.slice(i, end);
			i = end;
			continue;
		}

		text += line[i];
		i++;
	}

	return { text, inBlockComment };
}

function skipLiteral(line: string, start: number) {
	const quote = line[start];

	for (let i = start + 1; i < line.length; i++) {
		if (line[i] === '\\') {
			i++;
		} else if (line[i] === quote) {
			return i + 1;
		}
	}

	return line.length;
}
