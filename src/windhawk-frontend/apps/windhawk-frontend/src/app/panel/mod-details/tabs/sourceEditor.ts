/**
 * What the read-only CodeMirror views of a mod's source have in common: the
 * keys and search they answer to, the frame they draw in, and the words they
 * show in the app's language.
 */

import { standardKeymap } from '@codemirror/commands';
import {
  closeSearchPanel,
  openSearchPanel,
  search,
  searchKeymap,
} from '@codemirror/search';
import { EditorState, type Extension } from '@codemirror/state';
import { EditorView, highlightSpecialChars, keymap } from '@codemirror/view';
import type { TFunction } from 'i18next';
import {
  isLetterKey,
  isTypingTarget,
  useKeyboardShortcut,
} from '@app/panel/shared/useKeyboardShortcut';
import { sourceCodeTheme } from './sourceCodeTheme';
import { sourceSearchPanel } from './sourceSearchPanel';

// An Escape that closes the search goes no further, so it leaves open a dialog
// the view is in.
const sourceSearchKeymap = searchKeymap.map((binding) =>
  binding.run === closeSearchPanel ? { ...binding, stopPropagation: true } : binding
);

export const readOnlySource: Extension = [
  // Contenteditable, so the caret, keyboard selection and select-all work over
  // the whole text; read-only is what turns away every edit.
  EditorState.readOnly.of(true),
  highlightSpecialChars(),
  search({ createPanel: sourceSearchPanel }),
  keymap.of([...standardKeymap, ...sourceSearchKeymap]),
  sourceCodeTheme,
];

export function sourcePhrases(t: TFunction): Extension {
  return EditorState.phrases.of({
    Find: t('modDetails.code.editor.find'),
    next: t('modDetails.code.editor.next'),
    previous: t('modDetails.code.editor.previous'),
    'match case': t('modDetails.code.editor.matchCase'),
    regexp: t('modDetails.code.editor.regexp'),
    'by word': t('modDetails.code.editor.byWord'),
    close: t('modDetails.code.editor.close'),
    'current match': t('modDetails.code.editor.currentMatch'),
    'on line': t('modDetails.code.editor.onLine'),
    'Go to line': t('modDetails.code.editor.goToLine'),
    go: t('modDetails.code.editor.go'),
    'Control character': t('modDetails.code.editor.controlCharacter'),
  });
}

export function contentLabel(label: string): Extension {
  return EditorView.contentAttributes.of({ 'aria-label': label });
}

function isFindShortcut(event: KeyboardEvent) {
  return (
    (event.ctrlKey || event.metaKey) &&
    !event.altKey &&
    !event.shiftKey &&
    isLetterKey(event, 'f')
  );
}

/**
 * Ctrl+F opens the search of the view `target` names, from anywhere on the
 * page. The browser's own find would only see the lines that are drawn.
 */
export function useFindShortcut(target: () => EditorView | null | undefined) {
  // Inside a view, its search keymap has already answered the keystroke.
  useKeyboardShortcut(
    true,
    (event) =>
      isFindShortcut(event) && !event.defaultPrevented && !isTypingTarget(event),
    () => {
      const view = target();
      if (view) {
        openSearchPanel(view);
      }
    }
  );
}
