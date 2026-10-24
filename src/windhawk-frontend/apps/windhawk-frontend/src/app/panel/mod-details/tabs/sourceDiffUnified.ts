/**
 * A diff as one document: the lines a change removes followed by the lines it
 * adds, between the unchanged lines around them, with each side's line numbers
 * in a gutter of its own.
 *
 * A removed line is a line of the document like any other, so it is drawn a
 * window at a time, found by search, and selected and copied along with what
 * surrounds it. That document mixes two versions of the file and parses as
 * neither, so each line is highlighted from a parse of the version it comes
 * from: a line deep in the readme's block comment is drawn as the comment it
 * is.
 */

import { cppLanguage } from '@codemirror/lang-cpp';
import { ensureSyntaxTree, highlightingFor } from '@codemirror/language';
import {
  EditorSelection,
  EditorState,
  Prec,
  StateEffect,
  Text,
  type Extension,
  type Range,
} from '@codemirror/state';
import {
  Decoration,
  EditorView,
  GutterMarker,
  ViewPlugin,
  getDialog,
  gutter,
  keymap,
  showDialog,
  type Command,
  type DecorationSet,
  type ViewUpdate,
} from '@codemirror/view';
import type { Tree } from '@lezer/common';
import { highlightTree, type Tag } from '@lezer/highlight';
import { foldedGaps, gapsOf, type LineStretch } from './sourceDiffGaps';
import type { SourceDiff } from './sourceDiff';

export interface UnifiedDiff {
  doc: Text;
  // Each row's line number on the old and the new side, 1-based, or 0 on the
  // side the row is not from.
  oldLines: Int32Array;
  newLines: Int32Array;
  // The changed text of the removed and of the added rows, as from-to pairs in
  // document positions, in order.
  removedText: Int32Array;
  addedText: Int32Array;
  // The unchanged rows between changes.
  stretches: LineStretch[];
}

export function unifiedDiff({ oldDoc, newDoc, regions }: SourceDiff): UnifiedDiff {
  const rows: string[] = [];
  const oldLines: number[] = [];
  const newLines: number[] = [];
  const removedText: number[] = [];
  const addedText: number[] = [];
  const stretches: LineStretch[] = [];

  let position = 0;
  const addRow = (text: string, oldLine: number, newLine: number) => {
    rows.push(text);
    oldLines.push(oldLine);
    newLines.push(newLine);
    position += text.length + 1;
  };

  let oldLine = 0;
  let newLine = 0;
  const addUnchanged = (oldTo: number) => {
    const from = rows.length + 1;
    for (; oldLine < oldTo; oldLine++, newLine++) {
      addRow(newDoc.line(newLine + 1).text, oldLine + 1, newLine + 1);
    }
    stretches.push({ from, to: rows.length });
  };

  for (const region of regions) {
    addUnchanged(region.oldFrom);

    // A region's rows on each side are its lines in order, so a position in its
    // text on that side is the same distance into those rows.
    const removedStart = position;
    for (; oldLine < region.oldTo; oldLine++) {
      addRow(oldDoc.line(oldLine + 1).text, oldLine + 1, 0);
    }
    const addedStart = position;
    for (; newLine < region.newTo; newLine++) {
      addRow(newDoc.line(newLine + 1).text, 0, newLine + 1);
    }

    for (const edit of region.edits ?? []) {
      if (edit.fromA < edit.toA) {
        removedText.push(removedStart + edit.fromA, removedStart + edit.toA);
      }
      if (edit.fromB < edit.toB) {
        addedText.push(addedStart + edit.fromB, addedStart + edit.toB);
      }
    }
  }
  addUnchanged(oldDoc.lines);

  return {
    doc: Text.of(rows),
    oldLines: Int32Array.from(oldLines),
    newLines: Int32Array.from(newLines),
    removedText: Int32Array.from(removedText),
    addedText: Int32Array.from(addedText),
    stretches,
  };
}

/**
 * One version of the file, parsed as far as has been asked for and no further.
 */
export class SourceParse {
  private readonly state: EditorState;
  private tree: Tree | null = null;
  // How far the tree is known to be complete.
  private covered = 0;

  constructor(readonly doc: Text) {
    this.state = EditorState.create({ doc, extensions: cppLanguage });
  }

  // Whether the parse is complete as far as `upto`.
  covers(upto: number): boolean {
    return upto <= this.covered;
  }

  // The tree, when it is complete as far as `upto`.
  treeUpTo(upto: number): Tree | null {
    return this.covers(upto) ? this.tree : null;
  }

  // Parses on toward `upto` for at most `budget` milliseconds; whether it got
  // there. Work left unfinished is picked up by the next call.
  advance(upto: number, budget: number): boolean {
    if (this.covers(upto)) {
      return true;
    }
    const tree = ensureSyntaxTree(this.state, upto, budget);
    if (!tree) {
      return false;
    }
    this.tree = tree;
    this.covered = upto;
    return true;
  }
}

// How long a draw may parse toward the lines it draws, and how long each later
// slice of the parse may run, the page responding between slices.
const DRAW_PARSE_BUDGET = 20;
const IDLE_PARSE_SLICE = 20;

// A parse gained ground, so the lines it reached can be highlighted.
const parsed = StateEffect.define<null>();

const removedLine = Decoration.line({ class: 'cm-diff-removedLine' });
const addedLine = Decoration.line({ class: 'cm-diff-addedLine' });
const removedText = Decoration.mark({ class: 'cm-diff-removedText' });
const addedText = Decoration.mark({ class: 'cm-diff-addedText' });

// The first from-to pair in `ranges` that ends after `pos`.
function firstRangeEndingAfter(ranges: Int32Array, pos: number) {
  let low = 0;
  let high = ranges.length / 2;
  while (low < high) {
    const middle = (low + high) >> 1;
    if (ranges[middle * 2 + 1] <= pos) {
      low = middle + 1;
    } else {
      high = middle;
    }
  }
  return low;
}

function markRanges(
  out: Range<Decoration>[],
  ranges: Int32Array,
  mark: Decoration,
  from: number,
  to: number
) {
  for (let i = firstRangeEndingAfter(ranges, from); i * 2 < ranges.length; i++) {
    if (ranges[i * 2] >= to) {
      break;
    }
    const start = Math.max(from, ranges[i * 2]);
    const end = Math.min(to, ranges[i * 2 + 1]);
    if (start < end) {
      out.push(mark.range(start, end));
    }
  }
}

class UnifiedDecorations {
  decorations: DecorationSet = Decoration.none;
  private readonly marks = new Map<string, Decoration>();
  // How far into each version the lines in view reach.
  private needed = { old: 0, new: 0 };
  private scheduled: ReturnType<typeof setTimeout> | null = null;

  constructor(
    private readonly view: EditorView,
    private readonly diff: UnifiedDiff,
    private readonly parses: { old: SourceParse; new: SourceParse }
  ) {
    this.build();
  }

  update(update: ViewUpdate) {
    if (
      update.viewportChanged ||
      update.transactions.some(
        (tr) => tr.reconfigured || tr.effects.some((effect) => effect.is(parsed))
      )
    ) {
      this.build();
    }
  }

  destroy() {
    if (this.scheduled !== null) {
      clearTimeout(this.scheduled);
    }
  }

  private mark(className: string) {
    let mark = this.marks.get(className);
    if (!mark) {
      mark = Decoration.mark({ class: className });
      this.marks.set(className, mark);
    }
    return mark;
  }

  private build() {
    const { view, diff } = this;
    const { doc } = view.state;
    const highlighter = {
      style: (tags: readonly Tag[]) => highlightingFor(view.state, tags),
    };

    // An unchanged row is highlighted as the new version has it, that being the
    // version read toward.
    const lines: { from: number; to: number; row: number; side: 'old' | 'new' }[] = [];
    const needed = { old: 0, new: 0 };
    for (const { from, to } of view.visibleRanges) {
      for (let pos = from; pos <= to; ) {
        const line = doc.lineAt(pos);
        const row = line.number - 1;
        const side = diff.newLines[row] ? 'new' : 'old';
        const sourceLine = side === 'new' ? diff.newLines[row] : diff.oldLines[row];
        lines.push({ from: line.from, to: line.to, row, side });
        needed[side] = Math.max(needed[side], this.parses[side].doc.line(sourceLine).to);
        pos = line.to + 1;
      }
    }
    this.needed = needed;

    for (const side of ['old', 'new'] as const) {
      this.parses[side].advance(needed[side], DRAW_PARSE_BUDGET);
    }

    const ranges: Range<Decoration>[] = [];
    for (const { from, to, row, side } of lines) {
      if (!diff.newLines[row]) {
        ranges.push(removedLine.range(from));
        markRanges(ranges, diff.removedText, removedText, from, to);
      } else if (!diff.oldLines[row]) {
        ranges.push(addedLine.range(from));
        markRanges(ranges, diff.addedText, addedText, from, to);
      }

      const source = this.parses[side];
      const sourceLine = side === 'new' ? diff.newLines[row] : diff.oldLines[row];
      const sourceFrom = source.doc.line(sourceLine).from;
      const sourceTo = sourceFrom + (to - from);
      const tree = source.treeUpTo(sourceTo);
      if (tree) {
        highlightTree(
          tree,
          highlighter,
          (start, end, className) => {
            ranges.push(this.mark(className).range(from + start - sourceFrom, from + end - sourceFrom));
          },
          sourceFrom,
          sourceTo
        );
      }
    }

    this.decorations = Decoration.set(ranges, true);
    this.schedule();
  }

  // Parses on in slices, between which the page responds, until the lines in
  // view can all be highlighted.
  private schedule() {
    if (
      this.scheduled !== null ||
      (this.parses.old.covers(this.needed.old) && this.parses.new.covers(this.needed.new))
    ) {
      return;
    }
    this.scheduled = setTimeout(() => {
      this.scheduled = null;
      const done =
        this.parses.old.advance(this.needed.old, IDLE_PARSE_SLICE) &&
        this.parses.new.advance(this.needed.new, IDLE_PARSE_SLICE);
      if (done) {
        this.view.dispatch({ effects: parsed.of(null) });
      } else {
        this.schedule();
      }
    });
  }
}

class LineNumber extends GutterMarker {
  constructor(
    readonly number: number,
    override readonly elementClass: string
  ) {
    super();
  }

  override eq(other: LineNumber) {
    return other.number === this.number && other.elementClass === this.elementClass;
  }

  override toDOM() {
    return document.createTextNode(this.number ? String(this.number) : '');
  }
}

function lineNumberGutter(diff: UnifiedDiff, side: 'old' | 'new') {
  const numbers = side === 'old' ? diff.oldLines : diff.newLines;
  const widest = numbers.reduce((a, b) => Math.max(a, b), 0);
  return gutter({
    class: 'cm-lineNumbers',
    lineMarker(view, line) {
      const row = view.state.doc.lineAt(line.from).number - 1;
      const elementClass = !diff.newLines[row]
        ? 'cm-diff-removedGutter'
        : !diff.oldLines[row]
        ? 'cm-diff-addedGutter'
        : '';
      return new LineNumber(numbers[row], elementClass);
    },
    initialSpacer: () => new LineNumber(widest, ''),
  });
}

// The new version's line a row is at: its own, or for a removed row the one
// above it.
function newLineAt(diff: UnifiedDiff, row: number) {
  for (; row >= 0; row--) {
    if (diff.newLines[row]) {
      return diff.newLines[row];
    }
  }
  return 1;
}

// CodeMirror's go-to-line, taking the number in the new version's lines rather
// than the document's rows, which follow neither gutter past a change.
function gotoNewLine(diff: UnifiedDiff): Command {
  const lineCount = newLineAt(diff, diff.newLines.length - 1);
  return (view) => {
    const open = getDialog(view, 'cm-goto-line');
    if (open) {
      open.dom.querySelector<HTMLInputElement>('input[type=text]')?.select();
      return true;
    }
    const { state } = view;
    const startLine = newLineAt(diff, state.doc.lineAt(state.selection.main.head).number - 1);
    const { close, result } = showDialog(view, {
      class: 'cm-goto-line',
      label: state.phrase('Go to line'),
      input: { type: 'text', name: 'line', value: String(startLine) },
      focus: true,
      submitLabel: state.phrase('go'),
    });
    result.then((form) => {
      const field = form?.elements.namedItem('line') as HTMLInputElement | null;
      const match = field && /^([+-])?(\d+)?(:\d+)?(%)?$/.exec(field.value);
      if (!match) {
        view.dispatch({ effects: close });
        return;
      }
      const [, sign, ln, cl, percent] = match;
      let line = ln ? +ln : startLine;
      if (ln && percent) {
        let fraction = line / 100;
        if (sign) {
          fraction = fraction * (sign === '-' ? -1 : 1) + startLine / lineCount;
        }
        line = Math.round(lineCount * fraction);
      } else if (ln && sign) {
        line = line * (sign === '-' ? -1 : 1) + startLine;
      }
      const row = diff.newLines.indexOf(Math.max(1, Math.min(lineCount, line)));
      const docLine = state.doc.line(row + 1);
      const column = cl ? +cl.slice(1) : 0;
      const selection = EditorSelection.cursor(
        docLine.from + Math.max(0, Math.min(column, docLine.length))
      );
      view.dispatch({
        effects: [close, EditorView.scrollIntoView(selection.from, { y: 'center' })],
        selection,
      });
    });
    return true;
  };
}

export function unifiedDiffView(diff: UnifiedDiff, sources: SourceDiff): Extension {
  const parses = { old: new SourceParse(sources.oldDoc), new: new SourceParse(sources.newDoc) };
  return [
    lineNumberGutter(diff, 'old'),
    lineNumberGutter(diff, 'new'),
    Prec.high(keymap.of([{ key: 'Mod-Alt-g', run: gotoNewLine(diff) }])),
    // Lower decorations wrap higher ones, so search matches draw over the tint
    // of the changed text they fall in.
    Prec.lowest(
      ViewPlugin.define((view) => new UnifiedDecorations(view, diff, parses), {
        decorations: (plugin) => plugin.decorations,
      })
    ),
    foldedGaps(gapsOf(diff.stretches)),
  ];
}
