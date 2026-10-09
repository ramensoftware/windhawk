/**
 * The web addresses in a run of plain text, found the way a reader would take
 * them: an address runs to the next space, and punctuation that closes the
 * sentence around it is the sentence's rather than the address's.
 */

export type TextPart =
  | { kind: 'text'; text: string }
  | { kind: 'link'; url: string };

const ADDRESS = /https?:\/\/[^\s<>"]+/gi;

// Always the sentence's when it ends an address.
const TRAILING_PUNCTUATION = new Set(".,;:!?'*");

// A closing bracket ends an address only where the address opened one to match
// it, as a Wikipedia title does; otherwise it closes the text around it.
const OPENING_BRACKETS: Record<string, string> = { ')': '(', ']': '[' };

function count(text: string, char: string) {
  return text.split(char).length - 1;
}

function trimAddress(address: string): string {
  // How many of each closing bracket the address has beyond its opening ones.
  const unmatched = new Map<string, number>();
  for (const [closing, opening] of Object.entries(OPENING_BRACKETS)) {
    unmatched.set(closing, count(address, closing) - count(address, opening));
  }
  let end = address.length;
  for (; end > 0; end--) {
    const last = address[end - 1];
    const excess = unmatched.get(last) ?? 0;
    if (excess > 0) {
      unmatched.set(last, excess - 1);
    } else if (!TRAILING_PUNCTUATION.has(last)) {
      break;
    }
  }
  return address.slice(0, end);
}

/**
 * The text in order, cut into the runs between addresses and the addresses
 * themselves. Joined back together, the parts are the text.
 */
export function splitTextLinks(text: string): TextPart[] {
  const parts: TextPart[] = [];
  let textStart = 0;
  for (const match of text.matchAll(ADDRESS)) {
    const url = trimAddress(match[0]);
    // The scheme alone is no address to go to.
    if (/^https?:\/\/$/i.test(url)) {
      continue;
    }
    const start = match.index ?? 0;
    if (start > textStart) {
      parts.push({ kind: 'text', text: text.slice(textStart, start) });
    }
    parts.push({ kind: 'link', url });
    textStart = start + url.length;
  }
  if (textStart < text.length) {
    parts.push({ kind: 'text', text: text.slice(textStart) });
  }
  return parts;
}
