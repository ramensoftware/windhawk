/**
 * A diff as the two versions side by side, each line level with the same line
 * of the other, on CodeMirror's merge view.
 *
 * The merge view takes its changes from the diff rather than finding its own,
 * which on a large file would cost seconds. It does present them its own way,
 * folding an unchanged line or two between changes into the change around them.
 */

import { cpp } from '@codemirror/lang-cpp';
import { MergeView, type Chunk } from '@codemirror/merge';
import { RangeSetBuilder, StateEffect, type Extension, type Text } from '@codemirror/state';
import { GutterMarker, gutterLineClass, lineNumbers, type EditorView } from '@codemirror/view';
import { markWhileStuck } from '@app/panel/shared/markWhileStuck';
import { documentChanges, type SourceDiff } from './sourceDiff';
import { foldedGaps, gapsOf, mirrorUnfolding, type LineStretch } from './sourceDiffGaps';
import { topPanelsIn } from './sourcePanels';

class ChangedLine extends GutterMarker {
  constructor(override readonly elementClass: string) {
    super();
  }
}

const removedLine = new ChangedLine('cm-diff-removedGutter');
const addedLine = new ChangedLine('cm-diff-addedGutter');

type Side = 'a' | 'b';

// The lines a chunk covers on one side, as a 1-based half-open range.
function chunkLines(doc: Text, chunk: Chunk, side: Side): [number, number] {
  const from = side === 'a' ? chunk.fromA : chunk.fromB;
  const to = side === 'a' ? chunk.toA : chunk.toB;
  const first = doc.lineAt(from).number;
  if (from === to) {
    return [first, first];
  }
  return [first, to > doc.length ? doc.lines + 1 : doc.lineAt(to).number];
}

function unchangedStretches(doc: Text, chunks: readonly Chunk[], side: Side): LineStretch[] {
  const stretches: LineStretch[] = [];
  let next = 1;
  for (const chunk of chunks) {
    const [first, end] = chunkLines(doc, chunk, side);
    stretches.push({ from: next, to: first - 1 });
    next = end;
  }
  stretches.push({ from: next, to: doc.lines });
  return stretches;
}

function changedLineGutter(doc: Text, chunks: readonly Chunk[], side: Side): Extension {
  const marker = side === 'a' ? removedLine : addedLine;
  const builder = new RangeSetBuilder<GutterMarker>();
  for (const chunk of chunks) {
    const [first, end] = chunkLines(doc, chunk, side);
    for (let line = first; line < end; line++) {
      const { from } = doc.line(line);
      builder.add(from, from, marker);
    }
  }
  return gutterLineClass.of(builder.finish());
}

export interface SplitDiffView {
  old: EditorView;
  new: EditorView;
  destroy(): void;
}

export function splitDiffView(
  parent: HTMLElement,
  diff: SourceDiff,
  editors: { old: Extension; new: Extension }
): SplitDiffView {
  // Each side's top panels - its search - go in a bar over both sides, half
  // each, so a side that opens one moves down no further than the other does
  // and the two stay level.
  const panelBar = parent.appendChild(document.createElement('div'));
  panelBar.className = 'cm-diff-panels';

  const changes = documentChanges(diff);
  const merge = new MergeView({
    parent,
    a: {
      doc: diff.oldDoc,
      extensions: [editors.old, lineNumbers(), cpp(), topPanelsIn(panelBar)],
    },
    b: {
      doc: diff.newDoc,
      extensions: [editors.new, lineNumbers(), cpp(), topPanelsIn(panelBar)],
    },
    gutter: false,
    // A copy each time, since the merge view rewrites the list in place.
    diffConfig: { override: () => changes.slice() },
  });

  // Its chunks are what the two sides are aligned by, so the folds are cut
  // from them too, and stretch for stretch they match across the sides.
  const sides: [EditorView, Text, Side, () => EditorView][] = [
    [merge.a, diff.oldDoc, 'a', () => merge.b],
    [merge.b, diff.newDoc, 'b', () => merge.a],
  ];
  for (const [view, doc, side, other] of sides) {
    view.dispatch({
      effects: StateEffect.appendConfig.of([
        foldedGaps(gapsOf(unchangedStretches(doc, merge.chunks, side))),
        mirrorUnfolding(other),
        changedLineGutter(doc, merge.chunks, side),
      ]),
    });
  }

  const stopMarking = markWhileStuck(panelBar);

  return {
    old: merge.a,
    new: merge.b,
    destroy() {
      stopMarking();
      merge.destroy();
      panelBar.remove();
    },
  };
}
