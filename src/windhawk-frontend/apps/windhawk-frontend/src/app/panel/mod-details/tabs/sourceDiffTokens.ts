/**
 * Syntax highlighting for a source diff, as the per-line token trees
 * `react-diff-view` renders into its code cells.
 *
 * The library's own `tokenize` builds the same thing through the same stages -
 * highlight each whole file, cut the token stream into lines, let `markEdits`
 * mark the changed spans, fold each line back into a tree - but its line cut
 * rebuilds the whole array of lines once per token, which costs O(tokens x
 * lines). That is the bulk of the time a diff takes to appear, and it is paid
 * again on every expand: a few hundred ms of blocked main thread on a mod of a
 * few thousand lines, seconds on the largest ones. Cutting by appending instead
 * makes it linear, which measures as roughly 5x faster at 1000 lines and 10x at
 * 8000. `sourceDiffTokens.spec.ts` pins the output to the library's, token for
 * token.
 */

import Prism from 'prismjs';
import 'prismjs/components/prism-c';
import 'prismjs/components/prism-cpp';
import { markEdits } from 'react-diff-view';
import type {
  HunkData,
  HunkTokens,
  TokenNode,
  TokenPath,
} from 'react-diff-view';

// Mods are C++, and Prism needs `c` loaded for it.
const LANGUAGE = 'cpp';

// react-diff-view consumes highlighting as HAST (https://github.com/syntax-tree/hast),
// which is refractor's output shape rather than Prism's own.
function prismTokensToHast(tokens: (string | Prism.Token)[]): TokenNode[] {
  const nodes: TokenNode[] = [];

  for (const token of tokens) {
    if (typeof token === 'string') {
      nodes.push({ type: 'text', value: token });
      continue;
    }

    const className = Array.isArray(token.type)
      ? token.type.map((type) => `token ${type}`)
      : ['token', token.type].filter(Boolean);

    let children: TokenNode[];
    if (typeof token.content === 'string') {
      children = [{ type: 'text', value: token.content }];
    } else if (Array.isArray(token.content)) {
      children = prismTokensToHast(token.content);
    } else {
      children = [{ type: 'text', value: String(token.content) }];
    }

    nodes.push({
      type: 'element',
      tagName: 'span',
      properties: { className },
      children,
    });
  }

  return nodes;
}

/**
 * One whole source as a highlighted token tree, or as a single text node when
 * the grammar is missing.
 *
 * A mod is highlighted whole rather than hunk by hunk because its readme is one
 * long block comment: a grammar that starts mid-file has no way to know it is
 * inside one.
 */
export function highlightSource(source: string): TokenNode[] {
  const grammar = Prism.languages[LANGUAGE];
  if (!grammar) {
    return [{ type: 'text', value: source }];
  }

  return prismTokensToHast(Prism.tokenize(source, grammar));
}

// A path always ends in a text node. The library's node types spell out only
// `type`, leaving the rest to an index signature, so the text has to be named.
type TextLeaf = { type: string; value: string };

const clone = (path: TokenPath): TokenPath => path.map((node) => ({ ...node }));

/**
 * Flatten a highlighted tree into one path per text leaf, each path holding the
 * chain of tokens that enclose it.
 *
 * The flat form is what lets a marker cut highlighting at a character offset
 * that falls in the middle of a token: a path can be split and rewrapped where a
 * tree cannot.
 */
function treeToPaths(nodes: TokenNode[]): TokenPath[] {
  const paths: TokenPath[] = [];
  const enclosing: TokenPath = [];

  const walk = (node: TokenNode) => {
    if (!node.children) {
      paths.push([...clone(enclosing), { ...node }]);
      return;
    }

    const { children, ...withoutChildren } = node;
    enclosing.push(withoutChildren);
    for (const child of children) {
      walk(child);
    }
    enclosing.pop();
  };

  for (const node of nodes) {
    walk(node);
  }

  return paths;
}

/**
 * Group paths into the lines they belong to, splitting the ones whose leaf spans
 * a line break so that each side of the break stays under its own enclosing
 * tokens.
 */
function pathsToLines(paths: TokenPath[]): TokenPath[][] {
  const lines: TokenPath[][] = [[]];

  for (const path of paths) {
    const leaf = path[path.length - 1] as TextLeaf;
    const values = leaf.value.split('\n');

    if (values.length === 1) {
      lines[lines.length - 1].push(path);
      continue;
    }

    const enclosing = path.slice(0, -1);
    lines[lines.length - 1].push([...clone(enclosing), { ...leaf, value: values[0] }]);
    for (const value of values.slice(1)) {
      lines.push([[...clone(enclosing), { ...leaf, value }]]);
    }
  }

  return lines;
}

/**
 * Fold one line's paths back into the token tree the code cell renders.
 *
 * Neighbouring text leaves are joined so a line comes out as its highlighted
 * spans and the plain text between them, not as a run of one node per token the
 * marker happened to cut. Only text merges: an element is reached again only
 * after the path before it has filled it in, so it never matches its neighbour.
 */
function pathsToTree(paths: TokenPath[]): TokenNode[] {
  const root: TokenNode = { type: 'root', children: [] };

  for (const path of paths) {
    let parent = root;

    path.forEach((node, i) => {
      const siblings = parent.children as TokenNode[];
      const previous = siblings[siblings.length - 1];
      const isLeaf = i === path.length - 1;

      if (isLeaf && previous?.type === 'text' && node.type === 'text') {
        const { value } = previous as TextLeaf;
        const merged = { ...previous, value: value + (node as TextLeaf).value };
        siblings[siblings.length - 1] = merged;
        parent = merged;
        return;
      }

      const attached = isLeaf ? { ...node } : { ...node, children: [] };
      siblings.push(attached);
      parent = attached;
    });
  }

  return root.children ?? [];
}

/**
 * The token trees for both sides of a diff, indexed by line number - 1, with the
 * spans that differ within a changed line marked.
 *
 * Highlighting is a nicety over a diff that reads fine without it, so a grammar
 * or a marker that trips over the sources gives up rather than taking the diff
 * down with it.
 */
export function sourceDiffTokens(
  hunks: HunkData[],
  oldSource: string,
  newSource: string
): HunkTokens | undefined {
  try {
    const [oldLines, newLines] = markEdits(hunks, { type: 'block' })([
      pathsToLines(treeToPaths(highlightSource(oldSource))),
      pathsToLines(treeToPaths(highlightSource(newSource))),
    ]);

    return {
      old: oldLines.map(pathsToTree),
      new: newLines.map(pathsToTree),
    };
  } catch {
    return undefined;
  }
}
