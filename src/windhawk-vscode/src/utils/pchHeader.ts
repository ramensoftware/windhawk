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
	const closeGroup = (endLines: string[]) => {
		const group = openGroups.pop()!;
		const parent = currentGroup();
		if (group.hasContent) {
			parent.lines.push(...group.lines, ...endLines);
			parent.hasContent = true;
		}
	};

	let inBlockComment = false;

	for (const { raw, spliced } of logicalLines(modSource)) {
		const startedInBlockComment = inBlockComment;
		const code = stripComments(spliced, startedInBlockComment);
		inBlockComment = code.inBlockComment;

		const directive = startedInBlockComment ? null : directiveRegex.exec(code.text)?.[1];
		if (!directive) {
			continue;
		}

		// Copying the source lines verbatim leaves the header splicing them the
		// way the mod source does. A directive that opens a block comment
		// contributes only its code, so the comment doesn't reach the header
		// unterminated.
		const copied = code.inBlockComment
			? [code.text.trimEnd()]
			: raw.map((line) => line.trimEnd());

		if (copiedDirectives.includes(directive)) {
			currentGroup().lines.push(...copied);
			currentGroup().hasContent = true;
		} else if (conditionalOpeners.includes(directive)) {
			openGroups.push({ lines: copied, hasContent: false });
		} else if (conditionalBranches.includes(directive) && openGroups.length > 1) {
			currentGroup().lines.push(...copied);
		} else if (directive === 'endif' && openGroups.length > 1) {
			closeGroup(copied);
		}
	}

	// A conditional the source never closes (only reachable through a comment
	// the scan read differently than the compiler does) still has to come out as
	// a header that compiles.
	while (openGroups.length > 1) {
		closeGroup(['#endif']);
	}

	return root.lines;
}

// The source lines a trailing backslash joins into one, and the single line
// they splice into. Splicing comes before comments are recognized, so a
// backslash ending a // comment continues the line as much as one ending code
// does, and the comment swallows what follows.
function* logicalLines(modSource: string) {
	let raw: string[] = [];
	let spliced = '';

	for (const line of modSource.split(/\r?\n/)) {
		raw.push(line);

		const trimmed = line.trimEnd();
		if (trimmed.endsWith('\\')) {
			spliced += trimmed.slice(0, -1);
			continue;
		}

		yield { raw, spliced: spliced + line };
		raw = [];
		spliced = '';
	}

	if (raw.length > 0) {
		yield { raw, spliced };
	}
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
