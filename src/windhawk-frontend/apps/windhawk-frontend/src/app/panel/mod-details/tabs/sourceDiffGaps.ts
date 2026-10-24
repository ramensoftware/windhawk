/**
 * The unchanged stretches of a diff, folded behind a band of buttons that unfold
 * them: whole, or a step at a time from an end that meets a change.
 *
 * A stretch is numbered by its place among the diff's stretches, so the two
 * sides of a split view number theirs alike, and an unfolding on one is
 * repeated on the other.
 */

import { icon } from '@fortawesome/fontawesome-svg-core';
import {
  faArrowsAltV,
  faLongArrowAltDown,
  faLongArrowAltUp,
} from '@fortawesome/free-solid-svg-icons';
import {
  Annotation,
  EditorState,
  Facet,
  StateEffect,
  StateField,
  Transaction,
  type Extension,
} from '@codemirror/state';
import {
  Decoration,
  EditorView,
  WidgetType,
  type DecorationSet,
} from '@codemirror/view';

// Lines left in view on each side of a change.
export const CONTEXT_LINES = 3;

// Lines a step unfolds.
export const UNFOLD_STEP = 10;

export interface Gap {
  id: number;
  // The first and last line folded, 1-based.
  from: number;
  to: number;
  // Which ends meet a change, and so can be unfolded from a step at a time.
  place: 'start' | 'middle' | 'end';
}

// Down unfolds the lines under the change above the gap, up the lines over the
// change below it.
export type UnfoldDirection = 'down' | 'all' | 'up';

export interface LineStretch {
  // 1-based and inclusive; empty when `to` is below `from`.
  from: number;
  to: number;
}

/**
 * The gaps a document's unchanged stretches fold into, given every stretch
 * between its changes in order, empty ones included: the first above the first
 * change and the last below the last.
 */
export function gapsOf(stretches: readonly LineStretch[]): Gap[] {
  const gaps: Gap[] = [];
  stretches.forEach((stretch, id) => {
    // A change empty on this side at the top or bottom leaves the stretch next
    // to it on line 1 or the last line, meeting a change all the same.
    const atStart = id === 0;
    const atEnd = id === stretches.length - 1;
    const from = atStart ? stretch.from : stretch.from + CONTEXT_LINES;
    const to = atEnd ? stretch.to : stretch.to - CONTEXT_LINES;
    if (from <= to) {
      gaps.push({ id, from, to, place: atStart ? 'start' : atEnd ? 'end' : 'middle' });
    }
  });
  return gaps;
}

export function unfoldButtons({ from, to, place }: Gap): [UnfoldDirection, number][] {
  const lines = to - from + 1;
  const stepped = lines > UNFOLD_STEP;
  return [
    ...(stepped && place !== 'start' ? [['down', UNFOLD_STEP] as [UnfoldDirection, number]] : []),
    ['all', lines],
    ...(stepped && place !== 'end' ? [['up', UNFOLD_STEP] as [UnfoldDirection, number]] : []),
  ];
}

function unfold(gap: Gap, direction: UnfoldDirection): Gap[] {
  const rest =
    direction === 'all'
      ? null
      : direction === 'down'
      ? { ...gap, from: gap.from + UNFOLD_STEP }
      : { ...gap, to: gap.to - UNFOLD_STEP };
  return rest && rest.from <= rest.to ? [rest] : [];
}

export const unfoldGap = StateEffect.define<{ id: number; direction: UnfoldDirection }>();

// An unfolding repeated from the other side of a split view, so it is not sent
// back.
const mirrored = Annotation.define<boolean>();

// A button's text, given the lines it unfolds.
export const unfoldLabel = Facet.define<
  (lines: number) => string,
  (lines: number) => string
>({
  combine: (values) => values[0] ?? ((lines) => String(lines)),
});

const ICONS = {
  down: faLongArrowAltDown,
  all: faArrowsAltV,
  up: faLongArrowAltUp,
};

// A button's height, which the theme draws it at and the editor estimates a band
// by until it is drawn. A band drawn at another height changes the page's height
// as it scrolls into view, and Chromium drops the move of a scrollbar drag that
// meets such a change, leaving the thumb behind the pointer.
export const UNFOLD_BUTTON_HEIGHT = 25;

class GapWidget extends WidgetType {
  constructor(
    readonly gap: Gap,
    readonly label: (lines: number) => string
  ) {
    super();
  }

  override eq(other: GapWidget) {
    return (
      other.gap.id === this.gap.id &&
      other.gap.from === this.gap.from &&
      other.gap.to === this.gap.to &&
      other.label === this.label
    );
  }

  toDOM(view: EditorView) {
    const band = document.createElement('div');
    band.className = 'cm-diff-gap';
    for (const [direction, lines] of unfoldButtons(this.gap)) {
      const button = band.appendChild(document.createElement('button'));
      button.type = 'button';
      button.className = 'cm-diff-unfold';
      const glyph = icon(ICONS[direction]).node[0];
      if (glyph) {
        button.appendChild(glyph);
      }
      button.appendChild(document.createTextNode(this.label(lines)));
      button.addEventListener('click', () => {
        view.dispatch({ effects: unfoldGap.of({ id: this.gap.id, direction }) });
      });
    }
    return band;
  }

  override get estimatedHeight() {
    return unfoldButtons(this.gap).length * UNFOLD_BUTTON_HEIGHT;
  }
}

function decorate(state: EditorState, gaps: readonly Gap[]): DecorationSet {
  const label = state.facet(unfoldLabel);
  return Decoration.set(
    gaps.map((gap) =>
      Decoration.replace({ widget: new GapWidget(gap, label), block: true }).range(
        state.doc.line(gap.from).from,
        state.doc.line(gap.to).to
      )
    )
  );
}

interface Folded {
  gaps: readonly Gap[];
  decorations: DecorationSet;
}

const foldedField = StateField.define<Folded>({
  create: () => ({ gaps: [], decorations: Decoration.none }),
  update(folded, tr) {
    let { gaps } = folded;
    for (const effect of tr.effects) {
      if (effect.is(unfoldGap)) {
        const { id, direction } = effect.value;
        gaps = gaps.flatMap((gap) => (gap.id === id ? unfold(gap, direction) : [gap]));
      }
    }
    if (gaps === folded.gaps && tr.startState.facet(unfoldLabel) === tr.state.facet(unfoldLabel)) {
      return folded;
    }
    return { gaps, decorations: decorate(tr.state, gaps) };
  },
  provide: (field) => EditorView.decorations.from(field, (folded) => folded.decorations),
});

// A selection the view is sent to, rather than one made in it - a search match,
// a line gone to - unfolds the gaps it lands in, where it would not be drawn.
// Selecting all is made in the view, and leaves the folds alone.
const unfoldAtSelection = EditorState.transactionExtender.of((tr) => {
  if (
    !tr.selection ||
    !(tr.isUserEvent('select.search') || tr.annotation(Transaction.userEvent) === undefined)
  ) {
    return null;
  }
  const { doc } = tr.startState;
  const { from, to } = tr.selection.main;
  const fromLine = doc.lineAt(from).number;
  const toLine = doc.lineAt(to).number;
  const effects = tr.startState
    .field(foldedField)
    .gaps.filter((gap) => gap.from <= toLine && gap.to >= fromLine)
    .map((gap) => unfoldGap.of({ id: gap.id, direction: 'all' }));
  return effects.length ? { effects } : null;
});

export function foldedGaps(gaps: readonly Gap[]): Extension {
  return [
    foldedField.init((state) => ({ gaps, decorations: decorate(state, gaps) })),
    unfoldAtSelection,
  ];
}

export function foldedGapsOf(state: EditorState): readonly Gap[] {
  return state.field(foldedField, false)?.gaps ?? [];
}

// Repeats each unfolding on the other side of a split view.
export function mirrorUnfolding(other: () => EditorView): Extension {
  return EditorView.updateListener.of((update) => {
    for (const tr of update.transactions) {
      const effects = tr.effects.filter((effect) => effect.is(unfoldGap));
      if (effects.length && !tr.annotation(mirrored)) {
        other().dispatch({ effects, annotations: mirrored.of(true) });
      }
    }
  });
}
