// A source's lines, ended by any of CRLF, CR and LF, so a source saved on
// Windows reads as one saved elsewhere.
export const splitLines = (source: string) => source.split(/\r\n?|\n/);

export function sameLines(a: string, b: string) {
  if (a === b) {
    return true;
  }
  const aLines = splitLines(a);
  const bLines = splitLines(b);
  return aLines.length === bLines.length && aLines.every((line, i) => line === bLines[i]);
}
