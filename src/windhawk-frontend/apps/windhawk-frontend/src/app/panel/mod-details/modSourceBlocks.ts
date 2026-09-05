/**
 * The readme and settings blocks of a mod source: a `/*` comment wrapped in
 * `// ==<name>==` and `// ==/<name>==` marker lines.
 *
 * Scanned by hand rather than spelled out as one regex. The regex for this shape
 * ends up writing the comment as `\/\*\s*([\s\S]+?)\s*` before its terminator,
 * three quantifiers with a claim on the same whitespace: a block whose comment
 * never closes has no match to find, and proving that means trying every way to
 * divide the whitespace run after the `/*` between the leading `\s*`, the split
 * point and the trailing `\s*`. That is cubic in the run's length - 4 KB of it
 * costs 15 seconds, 8 KB two minutes, 25 KB an hour - and the website build
 * reads both blocks the moment the source arrives, so opening the mod's page is
 * the whole of it.
 *
 * Reads a block character for character as windhawk-core's `find_comment_block`
 * (domain/src/scan.rs) does, so a mod says the same thing on its page here and
 * in the app. The one place the two part is the byte order mark a Windows editor
 * may write at the head of a file: the host counts it as opening the first line,
 * and this does not, so the one marker line it can sit in front of goes unread.
 */

// The classes the host scan works in, neither of them a JS one.
// `char::is_whitespace` is Unicode `White_Space`, which `\s` and `trimEnd`
// differ from in both directions: they take U+FEFF and leave out U+0085. A line
// ends at a carriage return or a line feed alone, where a multiline `^` and `$`
// anchor at U+2028 and U+2029 as well - whitespace to the host, but not the end
// of a line.
const whitespace = /[^\S\ufeff]|\u0085/;
const lineTerminator = /[\n\r]/;

interface CommentBlock {
  /** Where the comment's body opens, just past its `/*`. */
  bodyStart: number;
  /** Where the comment's body ends, at the terminator that closes the block. */
  bodyEnd: number;
  /** The body with the whitespace either end of it taken off. */
  contentStart: number;
  contentEnd: number;
}

/**
 * The block the `// ==<name>==` and `// ==/<name>==` marker lines wrap, or null
 * when the source holds none of that name.
 *
 * The first opening marker line a comment follows owns the block, and the first
 * comment terminator after it that the closing marker line follows ends it - a
 * terminator without that line after it is content, which is how a readme gets
 * to write one mid-sentence. No later opening line can accept what that one
 * rejected: its content would start further along, leaving it a subset of the
 * same terminators to end on.
 *
 * The content covers at least one character, so a comment holding nothing but
 * whitespace is no block.
 */
function scanCommentBlock(source: string, name: string): CommentBlock | null {
  const openLine = new RegExp(String.raw`^//[ \t]+==${name}==[ \t]*$`, 'gm');
  const closeLine = new RegExp(String.raw`//[ \t]+==/${name}==[ \t]*$`, 'my');
  const whitespaceRun = new RegExp(`(?:${whitespace.source})*`, 'y');

  const skipWhitespace = (from: number) => {
    whitespaceRun.lastIndex = from;
    whitespaceRun.exec(source);
    return whitespaceRun.lastIndex;
  };

  const isLineStart = (at: number) =>
    at === 0 || lineTerminator.test(source[at - 1]);

  const isLineEnd = (at: number) =>
    at === source.length || lineTerminator.test(source[at]);

  // The closing marker line, opening a line of its own, past the whitespace
  // that starts at `from`.
  const closedAt = (from: number) => {
    const at = skipWhitespace(from);
    if (!isLineStart(at)) {
      return false;
    }
    closeLine.lastIndex = at;
    return closeLine.test(source) && isLineEnd(closeLine.lastIndex);
  };

  for (
    let open = openLine.exec(source);
    open !== null;
    open = openLine.exec(source)
  ) {
    // A line the regex anchored at a separator the host does not count is no
    // marker line, and passing it over passes over no other: a marker line
    // holds no line terminator for one to start after.
    if (!isLineStart(open.index) || !isLineEnd(openLine.lastIndex)) {
      continue;
    }

    const commentStart = skipWhitespace(openLine.lastIndex);
    if (!source.startsWith('/*', commentStart)) {
      continue;
    }

    const bodyStart = commentStart + 2;

    // The head whitespace comes off once: no terminator can start inside a run
    // of it, so where the content begins is the same whichever one ends it.
    const contentStart = skipWhitespace(bodyStart);

    for (
      let end = source.indexOf('*/', contentStart);
      end !== -1;
      end = source.indexOf('*/', end + 2)
    ) {
      if (end > contentStart && closedAt(end + 2)) {
        let contentEnd = end;
        while (
          contentEnd > contentStart &&
          whitespace.test(source[contentEnd - 1])
        ) {
          contentEnd--;
        }
        return { bodyStart, bodyEnd: end, contentStart, contentEnd };
      }
    }

    return null;
  }

  return null;
}

/**
 * The content of the comment between the `// ==<name>==` and `// ==/<name>==`
 * marker lines, trimmed of whitespace on both sides, or null when the source
 * holds no such block.
 */
export function findCommentBlock(source: string, name: string): string | null {
  const block = scanCommentBlock(source, name);
  return block && source.slice(block.contentStart, block.contentEnd);
}

/**
 * Where the comment of that block sits in the source: `[start, end)`, opening
 * past the `/*` and ending at the terminator the closing marker line follows.
 * Null when the source holds no such block.
 */
export function findCommentBlockBody(
  source: string,
  name: string
): { start: number; end: number } | null {
  const block = scanCommentBlock(source, name);
  return block && { start: block.bodyStart, end: block.bodyEnd };
}
