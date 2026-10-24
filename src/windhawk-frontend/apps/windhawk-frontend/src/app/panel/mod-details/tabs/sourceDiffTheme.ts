/**
 * How a diff draws over the source view's frame: the tints of removed and added
 * lines and of the text changed in them, the line numbers beside them, and the
 * bands unchanged lines are folded behind. The tints are the theme's own tokens,
 * so one set of rules serves both themes.
 *
 * The unified view marks its rows with classes of its own; the split view's
 * come from the merge view, its old side being editor a and its new side b.
 */

import { EditorView } from '@codemirror/view';
import { UNFOLD_BUTTON_HEIGHT } from './sourceDiffGaps';

export const sourceDiffTheme = EditorView.theme({
  // Clips the gutter, which reaches the frame's left corners, to their radius.
  '.cm-scroller': {
    borderRadius: 'inherit',
  },
  '.cm-gutters': {
    color: 'var(--whui-text-muted)',
    backgroundColor: 'var(--whui-background-color)',
    borderRight: '1px solid var(--whui-border)',
  },
  '.cm-lineNumbers .cm-gutterElement': {
    padding: '0 8px',
  },

  '.cm-diff-removedLine, &.cm-merge-a .cm-changedLine': {
    backgroundColor: 'var(--whui-diff-code-delete)',
  },
  '.cm-diff-addedLine, &.cm-merge-b .cm-changedLine': {
    backgroundColor: 'var(--whui-diff-code-insert)',
  },
  '.cm-diff-removedText, &.cm-merge-a .cm-changedText': {
    background: 'var(--whui-diff-code-delete-edit)',
  },
  '.cm-diff-addedText, &.cm-merge-b .cm-changedText': {
    background: 'var(--whui-diff-code-insert-edit)',
  },
  '.cm-diff-removedGutter': {
    backgroundColor: 'var(--whui-diff-gutter-delete)',
  },
  '.cm-diff-addedGutter': {
    backgroundColor: 'var(--whui-diff-gutter-insert)',
  },
  // Where the other side has lines this one does not.
  '.cm-mergeSpacer': {
    backgroundColor: 'var(--whui-diff-omit)',
  },

  // Left-aligned, so the label stays in view while the code is scrolled
  // sideways no further than its start.
  '.cm-diff-unfold': {
    display: 'flex',
    alignItems: 'center',
    gap: '8px',
    boxSizing: 'border-box',
    width: '100%',
    height: `${UNFOLD_BUTTON_HEIGHT}px`,
    padding: '0 12px',
    font: 'inherit',
    color: 'var(--whui-text-secondary)',
    backgroundColor: 'var(--whui-chip-bg)',
    border: 'none',
    borderTop: '1px solid var(--whui-border)',
    borderBottom: '1px solid var(--whui-border)',
    cursor: 'pointer',
  },
  '.cm-diff-unfold + .cm-diff-unfold': {
    borderTop: 'none',
  },
  '.cm-diff-unfold:hover, .cm-diff-unfold:focus-visible': {
    color: 'var(--whui-primary)',
    outline: 'none',
  },
  '.cm-diff-unfold svg': {
    height: '1em',
  },
});
