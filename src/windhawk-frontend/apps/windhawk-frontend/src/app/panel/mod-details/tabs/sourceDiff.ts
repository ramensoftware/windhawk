/**
 * What differs between two versions of a mod's source: the runs of lines that
 * changed, and within a run that has lines on both sides, the text that did.
 *
 * Lines are matched patience-style. A line that occurs exactly once on each side
 * anchors the match, the longest run of anchors in the same order on both sides
 * splits the files into stretches, and each stretch is matched the same way in
 * turn. Only a stretch with nothing unique left in it is diffed exhaustively.
 * Most lines of a mod are unique, so on a file of 70,000 lines changed all over
 * this takes tens of milliseconds, where an exhaustive diff of the whole takes
 * seconds and jsdiff over half a minute.
 */

import diffSequences from '@jest/diff-sequences';
import { Change, presentableDiff } from '@codemirror/merge';
import { Text } from '@codemirror/state';
import { splitLines } from './sourceLines';

export interface DiffRegion {
  // The lines each side holds in place of the other's, 0-based and half-open.
  // One side is empty for a pure insertion or deletion.
  oldFrom: number;
  oldTo: number;
  newFrom: number;
  newTo: number;
  // The changed text, relative to the start of the region's first line on each
  // side, or null when the lines are changed whole.
  edits: readonly Change[] | null;
}

export interface SourceDiff {
  oldDoc: Text;
  newDoc: Text;
  regions: readonly DiffRegion[];
}

// Past this depth a stretch is diffed exhaustively rather than searched for
// anchors again. Real files are done long before it; it bounds what a crafted
// one can make the search cost.
const MAX_ANCHOR_DEPTH = 32;

// What exhaustive diffing may cost over the whole diff, as old lines times new
// lines summed over the stretches it runs on. A stretch past what is left
// is taken as replaced whole: it has no line unique to both sides, so what the
// two have in common is lines like braces and blanks.
const EXHAUSTIVE_BUDGET = 25_000_000;

// The most text a region can hold, both sides together, for its changed text to
// be marked. Inside a larger one, which lines are new already says it.
const MAX_REFINED_LENGTH = 20_000;

// The merge view's own default, which keeps a region's comparison quick
// however much of it differs.
const REFINE_SCAN_LIMIT = 500;

// A run of lines the two sides have in common: where it starts on each, and
// how long it is.
type Match = [oldStart: number, newStart: number, length: number];

type Task =
  | { match: Match }
  | {
      oldFrom: number;
      oldTo: number;
      newFrom: number;
      newTo: number;
      depth: number;
    };

/**
 * The runs of lines two sequences share, in order. Each line is given as a
 * number, equal for equal lines and below `idCount`.
 */
export function matchLines(
  oldIds: Int32Array,
  newIds: Int32Array,
  idCount: number
): Match[] {
  const matches: Match[] = [];
  const emit = (oldStart: number, newStart: number, length: number) => {
    if (!length) {
      return;
    }
    const last = matches[matches.length - 1];
    if (last && last[0] + last[2] === oldStart && last[1] + last[2] === newStart) {
      last[2] += length;
    } else {
      matches.push([oldStart, newStart, length]);
    }
  };

  const oldCount = new Int32Array(idCount);
  const newCount = new Int32Array(idCount);
  const newPosition = new Int32Array(idCount);
  let budget = EXHAUSTIVE_BUDGET;

  // Worked off a stack rather than by recursion, which a long file would take
  // past the call stack's depth. Tasks go on in reverse so they come off in
  // order, and matches are emitted in order with them.
  const stack: Task[] = [
    { oldFrom: 0, oldTo: oldIds.length, newFrom: 0, newTo: newIds.length, depth: 0 },
  ];
  while (stack.length) {
    const task = stack.pop() as Task;
    if ('match' in task) {
      emit(...task.match);
      continue;
    }

    let { oldFrom, oldTo, newFrom, newTo } = task;

    let prefix = 0;
    while (
      oldFrom + prefix < oldTo &&
      newFrom + prefix < newTo &&
      oldIds[oldFrom + prefix] === newIds[newFrom + prefix]
    ) {
      prefix++;
    }
    emit(oldFrom, newFrom, prefix);
    oldFrom += prefix;
    newFrom += prefix;

    let suffix = 0;
    while (
      oldTo - suffix > oldFrom &&
      newTo - suffix > newFrom &&
      oldIds[oldTo - 1 - suffix] === newIds[newTo - 1 - suffix]
    ) {
      suffix++;
    }
    oldTo -= suffix;
    newTo -= suffix;
    const tail: Task = { match: [oldTo, newTo, suffix] };

    if (oldFrom === oldTo || newFrom === newTo) {
      stack.push(tail);
      continue;
    }

    const anchors: number[] = [];
    if (task.depth < MAX_ANCHOR_DEPTH) {
      for (let i = oldFrom; i < oldTo; i++) {
        oldCount[oldIds[i]]++;
      }
      for (let j = newFrom; j < newTo; j++) {
        newCount[newIds[j]]++;
        newPosition[newIds[j]] = j;
      }
      for (let i = oldFrom; i < oldTo; i++) {
        const id = oldIds[i];
        if (oldCount[id] === 1 && newCount[id] === 1) {
          anchors.push(i);
        }
      }
      // Cleared over the same lines, which costs what counting did rather than
      // what the whole table would.
      for (let i = oldFrom; i < oldTo; i++) {
        oldCount[oldIds[i]] = 0;
      }
      for (let j = newFrom; j < newTo; j++) {
        newCount[newIds[j]] = 0;
      }
    }

    if (!anchors.length) {
      stack.push(tail);
      const cost = (oldTo - oldFrom) * (newTo - newFrom);
      if (cost <= budget) {
        budget -= cost;
        const found: Match[] = [];
        diffSequences(
          oldTo - oldFrom,
          newTo - newFrom,
          (i, j) => oldIds[oldFrom + i] === newIds[newFrom + j],
          (length, i, j) => found.push([oldFrom + i, newFrom + j, length])
        );
        for (let k = found.length - 1; k >= 0; k--) {
          stack.push({ match: found[k] });
        }
      }
      continue;
    }

    // The longest run of anchors in the same order on both sides, by patience
    // sorting: anchors in old order, each placed on the first pile whose top is
    // further along the new side than it, linked to the top of the pile before.
    const piles: number[] = [];
    const previous = new Int32Array(anchors.length);
    for (let k = 0; k < anchors.length; k++) {
      const position = newPosition[oldIds[anchors[k]]];
      let low = 0;
      let high = piles.length;
      while (low < high) {
        const middle = (low + high) >> 1;
        if (newPosition[oldIds[anchors[piles[middle]]]] < position) {
          low = middle + 1;
        } else {
          high = middle;
        }
      }
      previous[k] = low ? piles[low - 1] : -1;
      piles[low] = k;
    }
    const chain: number[] = [];
    for (let k = piles[piles.length - 1]; k >= 0; k = previous[k]) {
      chain.push(anchors[k]);
    }
    chain.reverse();

    const tasks: Task[] = [];
    let oldAt = oldFrom;
    let newAt = newFrom;
    for (const oldAnchor of chain) {
      const newAnchor = newPosition[oldIds[oldAnchor]];
      tasks.push(
        { oldFrom: oldAt, oldTo: oldAnchor, newFrom: newAt, newTo: newAnchor, depth: task.depth + 1 },
        { match: [oldAnchor, newAnchor, 1] }
      );
      oldAt = oldAnchor + 1;
      newAt = newAnchor + 1;
    }
    tasks.push(
      { oldFrom: oldAt, oldTo, newFrom: newAt, newTo, depth: task.depth + 1 },
      tail
    );
    for (let k = tasks.length - 1; k >= 0; k--) {
      stack.push(tasks[k]);
    }
  }

  return matches;
}

function lineIds(oldLines: string[], newLines: string[]) {
  const ids = new Map<string, number>();
  const idOf = (line: string) => {
    let id = ids.get(line);
    if (id === undefined) {
      id = ids.size;
      ids.set(line, id);
    }
    return id;
  };
  const oldIds = Int32Array.from(oldLines, idOf);
  const newIds = Int32Array.from(newLines, idOf);
  return { oldIds, newIds, idCount: ids.size };
}

export function diffSources(oldSource: string, newSource: string): SourceDiff {
  const oldLines = splitLines(oldSource);
  const newLines = splitLines(newSource);
  const { oldIds, newIds, idCount } = lineIds(oldLines, newLines);

  const regions: DiffRegion[] = [];
  const addRegion = (oldFrom: number, oldTo: number, newFrom: number, newTo: number) => {
    if (oldFrom === oldTo && newFrom === newTo) {
      return;
    }

    let edits: readonly Change[] | null = null;
    if (oldFrom < oldTo && newFrom < newTo) {
      const oldText = oldLines.slice(oldFrom, oldTo).join('\n');
      const newText = newLines.slice(newFrom, newTo).join('\n');
      if (oldText.length + newText.length <= MAX_REFINED_LENGTH) {
        edits = presentableDiff(oldText, newText, { scanLimit: REFINE_SCAN_LIMIT });
      }
    }
    regions.push({ oldFrom, oldTo, newFrom, newTo, edits });
  };

  let oldLine = 0;
  let newLine = 0;
  for (const [oldStart, newStart, length] of matchLines(oldIds, newIds, idCount)) {
    addRegion(oldLine, oldStart, newLine, newStart);
    oldLine = oldStart + length;
    newLine = newStart + length;
  }
  addRegion(oldLine, oldLines.length, newLine, newLines.length);

  return { oldDoc: Text.of(oldLines), newDoc: Text.of(newLines), regions };
}

// Where a line starts, the line one past the last included, as a document
// whose last line ended in a break would have it.
function lineStart(doc: Text, index: number) {
  return index < doc.lines ? doc.line(index + 1).from : doc.length + 1;
}

/**
 * The diff as changes between the two documents, in positions of each, which is
 * the form the merge view takes its diff in.
 */
export function documentChanges({ oldDoc, newDoc, regions }: SourceDiff): Change[] {
  const changes: Change[] = [];
  for (const region of regions) {
    let fromA = lineStart(oldDoc, region.oldFrom);
    let toA = lineStart(oldDoc, region.oldTo);
    let fromB = lineStart(newDoc, region.newFrom);
    let toB = lineStart(newDoc, region.newTo);

    if (region.edits) {
      for (const edit of region.edits) {
        changes.push(
          new Change(fromA + edit.fromA, fromA + edit.toA, fromB + edit.fromB, fromB + edit.toB)
        );
      }
      continue;
    }

    // Lines changed whole take the break after them, except at the end of a
    // document, which has none: there they take the break before them.
    if (toA > oldDoc.length || toB > newDoc.length) {
      if (fromA > 0 && fromB > 0) {
        fromA--;
        toA--;
        fromB--;
        toB--;
      }
      toA = Math.min(toA, oldDoc.length);
      toB = Math.min(toB, newDoc.length);
    }
    changes.push(new Change(fromA, toA, fromB, toB));
  }
  return changes;
}
