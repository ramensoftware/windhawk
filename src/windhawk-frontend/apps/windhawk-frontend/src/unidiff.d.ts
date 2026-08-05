// unidiff ships no types. Declared here for the part of it the source diff uses:
// a line diff, and its rendering as unified diff text for react-diff-view to
// parse back. `Change` is jsdiff's, which unidiff passes through untouched.
declare module 'unidiff' {
  export interface Change {
    value: string;
    count?: number;
    added?: boolean;
    removed?: boolean;
  }

  export function diffLines(a: string, b: string): Change[];

  export interface FormatLinesOptions {
    aname?: string;
    bname?: string;
    context?: number;
    pre_context?: number;
    post_context?: number;
  }

  export function formatLines(
    changes: Change[],
    options?: FormatLinesOptions
  ): string;
}
