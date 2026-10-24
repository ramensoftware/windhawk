/**
 * The search panel of the read-only source views: CodeMirror's own, less
 * replacing and selecting every match, with a count of the matches and of which
 * one is selected, and selecting the first match as the query is typed.
 */

import { icon } from '@fortawesome/fontawesome-svg-core';
import { faXmark } from '@fortawesome/free-solid-svg-icons';
import {
  SearchQuery,
  closeSearchPanel,
  findNext,
  findPrevious,
  getSearchQuery,
  setSearchQuery,
} from '@codemirror/search';
import { EditorSelection, type EditorState, type Text } from '@codemirror/state';
import {
  EditorView,
  runScopeHandlers,
  type Panel,
  type ViewUpdate,
} from '@codemirror/view';

// The document is counted in slices of about this many characters, as many to
// a turn as fit in COUNT_TURN_MS before the page gets its own: counting the
// largest source in the catalog whole takes the better part of a second.
const COUNT_SLICE_CHARS = 16384;
const COUNT_TURN_MS = 8;

// Slices end at line ends, so a match only runs past one by matching a line
// break. A string's match is no longer than the string; a regular expression's
// is looked for this far past the slice.
const REGEXP_REACH = 1000;

// The user event of the selections the panel makes as the query is typed.
const INCREMENTAL_SEARCH = 'select.search.incremental';

// Where each match of a query starts and ends, counted a slice at a time.
class Matches {
  done = false;
  // Pairs of match start and end, in document order.
  private readonly bounds: number[] = [];
  private pos = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private onTurn: (() => void) | null = null;

  constructor(
    readonly query: SearchQuery,
    private readonly state: EditorState
  ) {}

  // Counts the first turn's worth now, and the rest in turns of their own,
  // calling back after each of those.
  count(onTurn: () => void) {
    this.onTurn = onTurn;
    this.advance();
  }

  get doc(): Text {
    return this.state.doc;
  }

  get total() {
    return this.bounds.length / 2;
  }

  // The 1-based place of the match that spans exactly from-to, or 0.
  placeOf(from: number, to: number) {
    const index = this.indexFrom(from);
    return this.bounds[index * 2] === from && this.bounds[index * 2 + 1] === to
      ? index + 1
      : 0;
  }

  // The first match that starts at or after pos, else the first of all: null
  // if there is none, undefined while the count has yet to get that far.
  firstFrom(pos: number): { from: number; to: number } | null | undefined {
    let index = this.indexFrom(pos);
    if (index === this.total) {
      if (!this.done) {
        return undefined;
      }
      if (!this.total) {
        return null;
      }
      index = 0;
    }
    return { from: this.bounds[index * 2], to: this.bounds[index * 2 + 1] };
  }

  cancel() {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }

  // The index of the first match counted so far that starts at or after pos.
  private indexFrom(pos: number) {
    let low = 0;
    let high = this.total;
    while (low < high) {
      const mid = (low + high) >> 1;
      if (this.bounds[mid * 2] < pos) {
        low = mid + 1;
      } else {
        high = mid;
      }
    }
    return low;
  }

  private advance() {
    const { doc } = this.state;
    const reach = this.query.regexp ? REGEXP_REACH : this.query.search.length;
    const turnEnd = performance.now() + COUNT_TURN_MS;
    while (this.pos < doc.length && performance.now() < turnEnd) {
      const end = doc.lineAt(Math.min(doc.length, this.pos + COUNT_SLICE_CHARS)).to;
      const cursor = this.query.getCursor(
        this.state,
        this.pos,
        Math.min(doc.length, end + reach)
      );
      // A match that starts in the slice is its own, wherever it ends, and the
      // next slice starts past it.
      let next = end;
      for (let match = cursor.next(); !match.done; match = cursor.next()) {
        if (match.value.from >= end) {
          break;
        }
        this.bounds.push(match.value.from, match.value.to);
        next = Math.max(next, match.value.to);
      }
      this.pos = next;
    }

    if (this.pos < doc.length) {
      this.timer = setTimeout(() => {
        this.timer = null;
        this.advance();
        this.onTurn?.();
      });
    } else {
      this.done = true;
    }
  }
}

function button(name: string, content: string | Node, onClick: () => void) {
  const element = document.createElement('button');
  element.type = 'button';
  element.className = 'cm-button';
  element.name = name;
  element.append(content);
  element.onclick = onClick;
  return element;
}

function checkbox(name: string, checked: boolean, onChange: () => void) {
  const element = document.createElement('input');
  element.type = 'checkbox';
  element.name = name;
  element.checked = checked;
  element.onchange = onChange;
  return element;
}

function labeled(box: HTMLInputElement, label: string) {
  const element = document.createElement('label');
  element.append(box, label);
  return element;
}

class SourceSearchPanel implements Panel {
  readonly dom: HTMLElement;
  readonly top = true;
  private query: SearchQuery;
  private readonly searchField: HTMLInputElement;
  private readonly caseField: HTMLInputElement;
  private readonly reField: HTMLInputElement;
  private readonly wordField: HTMLInputElement;
  private readonly countLabel: HTMLElement;
  private matches: Matches | null = null;
  // Where a query being typed is looked for from: the start of the selection
  // as last made by anything but the typing.
  private searchFrom: number;
  // Whether the query last typed has yet to have its first match selected.
  private seeking = false;

  constructor(private readonly view: EditorView) {
    const query = (this.query = getSearchQuery(view.state));
    this.searchFrom = view.state.selection.main.from;
    const phrase = (text: string) => view.state.phrase(text);
    const commit = () => this.commit();

    const searchField = (this.searchField = document.createElement('input'));
    searchField.className = 'cm-textfield';
    searchField.name = 'search';
    searchField.autocomplete = 'off';
    searchField.value = query.search;
    searchField.placeholder = phrase('Find');
    searchField.setAttribute('aria-label', phrase('Find'));
    // What CodeMirror focuses when the panel is asked for again.
    searchField.setAttribute('main-field', 'true');
    searchField.onchange = commit;
    searchField.onkeyup = commit;

    this.caseField = checkbox('case', query.caseSensitive, commit);
    this.reField = checkbox('re', query.regexp, commit);
    this.wordField = checkbox('word', query.wholeWord, commit);

    this.countLabel = document.createElement('span');
    this.countLabel.className = 'cm-search-count';

    const close = button('close', icon(faXmark).node[0], () => closeSearchPanel(view));
    close.setAttribute('aria-label', phrase('close'));

    this.dom = document.createElement('div');
    this.dom.className = 'cm-search';
    this.dom.onkeydown = (event) => this.keydown(event);
    this.dom.append(
      searchField,
      this.countLabel,
      button('next', phrase('next'), () => findNext(view)),
      button('prev', phrase('previous'), () => findPrevious(view)),
      labeled(this.caseField, phrase('match case')),
      labeled(this.reField, phrase('regexp')),
      labeled(this.wordField, phrase('by word')),
      close
    );

    this.count(view.state);
  }

  mount() {
    this.searchField.select();
  }

  update(update: ViewUpdate) {
    for (const tr of update.transactions) {
      for (const effect of tr.effects) {
        if (effect.is(setSearchQuery) && !effect.value.eq(this.query)) {
          this.setQuery(effect.value);
        }
      }
      if (tr.selection && !tr.isUserEvent(INCREMENTAL_SEARCH)) {
        this.searchFrom = tr.selection.main.from;
        this.seeking = false;
      }
    }

    const { matches } = this;
    if (
      !matches ||
      matches.doc !== update.state.doc ||
      !matches.query.eq(getSearchQuery(update.state))
    ) {
      this.count(update.state);
    } else if (update.selectionSet) {
      this.showCount();
    }
  }

  destroy() {
    this.matches?.cancel();
  }

  private commit() {
    const query = new SearchQuery({
      search: this.searchField.value,
      caseSensitive: this.caseField.checked,
      regexp: this.reField.checked,
      wholeWord: this.wordField.checked,
    });
    if (!query.eq(this.query)) {
      this.query = query;
      this.view.dispatch({ effects: setSearchQuery.of(query) });
      // The count's first turn ran in that update, which can't be dispatched
      // from, so it called nothing back.
      this.seeking = true;
      this.seek();
    }
  }

  // Selects the first match from searchFrom once the count has found it.
  private seek() {
    if (!this.seeking) {
      return;
    }
    const match = this.matches ? this.matches.firstFrom(this.searchFrom) : null;
    if (match === undefined) {
      return;
    }
    this.seeking = false;
    if (match) {
      const selection = EditorSelection.single(match.from, match.to);
      this.view.dispatch({
        selection,
        effects: EditorView.scrollIntoView(selection.main),
        userEvent: INCREMENTAL_SEARCH,
      });
    }
  }

  private setQuery(query: SearchQuery) {
    this.query = query;
    this.searchField.value = query.search;
    this.caseField.checked = query.caseSensitive;
    this.reField.checked = query.regexp;
    this.wordField.checked = query.wholeWord;
  }

  private keydown(event: KeyboardEvent) {
    if (runScopeHandlers(this.view, event, 'search-panel')) {
      event.preventDefault();
    } else if (event.key === 'Enter' && event.target === this.searchField) {
      event.preventDefault();
      (event.shiftKey ? findPrevious : findNext)(this.view);
    }
  }

  private count(state: EditorState) {
    this.matches?.cancel();
    const query = getSearchQuery(state);
    this.matches = query.valid ? new Matches(query, state) : null;
    this.matches?.count(() => {
      this.showCount();
      this.seek();
    });
    this.showCount();
  }

  // A count under way leaves the last one in place, marked stale, so the panel
  // keeps its width. A query that matches nothing for being empty or not a
  // regular expression shows none.
  private showCount() {
    const { matches, countLabel } = this;
    countLabel.classList.toggle('cm-search-count-stale', !!matches && !matches.done);
    if (!matches) {
      countLabel.textContent = '';
    } else if (matches.done) {
      const { main } = this.view.state.selection;
      countLabel.textContent = `${matches.placeOf(main.from, main.to)}/${matches.total}`;
    }
  }
}

export function sourceSearchPanel(view: EditorView): Panel {
  return new SourceSearchPanel(view);
}
