/**
 * How the views of a mod's source draw: the editor's frame in the app's own
 * colors, and a highlight palette per theme - VS Code's Dark+ under the dark
 * theme, Prism's default under the light one.
 */

import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import type { Extension } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';

// antd's own stack for <pre> and <code>.
const CODE_FONT_FAMILY =
  "'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, Courier, monospace";

const darkHighlightStyle = HighlightStyle.define([
  {
    tag: [
      t.keyword,
      t.bool,
      t.standard(t.typeName),
      t.special(t.name),
      t.processingInstruction,
    ],
    color: '#569cd6',
  },
  { tag: [t.typeName, t.namespace], color: '#4ec9b0' },
  {
    tag: [t.function(t.variableName), t.function(t.propertyName)],
    color: '#dcdcaa',
  },
  { tag: t.string, color: '#ce9178' },
  { tag: t.escape, color: '#d7ba7d' },
  { tag: [t.number, t.literal], color: '#b5cea8' },
  { tag: t.comment, color: '#6a9955' },
  { tag: t.meta, color: '#9cdcfe' },
]);

const lightHighlightStyle = HighlightStyle.define([
  {
    tag: [t.keyword, t.standard(t.typeName), t.processingInstruction],
    color: '#0077aa',
  },
  {
    tag: [
      t.typeName,
      t.function(t.variableName),
      t.function(t.propertyName),
    ],
    color: '#dd4a68',
  },
  { tag: [t.string, t.escape], color: '#669900' },
  {
    tag: [t.number, t.literal, t.bool, t.null, t.special(t.name), t.meta],
    color: '#990055',
  },
  { tag: t.operator, color: '#9a6e3a' },
  { tag: t.comment, color: '#708090' },
]);

export const sourceCodeTheme = EditorView.theme({
  '&': {
    color: 'var(--whui-editor-fg)',
    backgroundColor: 'var(--whui-background-color)',
    borderRadius: 'var(--whui-radius)',
    fontSize: '13px',
  },
  '&.cm-focused': {
    outline: 'none',
  },
  '.cm-scroller': {
    fontFamily: CODE_FONT_FAMILY,
    lineHeight: '1.5',
  },
  '.cm-content': {
    padding: '12px 0',
    caretColor: 'var(--whui-editor-fg)',
  },
  '.cm-line': {
    padding: '0 12px',
  },
  '.cm-cursor': {
    borderLeftColor: 'var(--whui-editor-fg)',
  },

  // The search panel sticks to the top of the region the page scrolls in, so it
  // draws over the code and needs a surface of its own: the background around
  // the view, which --source-surface names where it is not a card's.
  '.cm-panels': {
    color: 'inherit',
    backgroundColor: 'var(--source-surface, var(--whui-card-background-color))',
  },
  // A row of controls 24px high, wrapping as the view narrows. The right end
  // holds the close button, and --source-search-inset more is left for whatever
  // the host hangs over that corner. The selectors match the search package's
  // own, which outrank a plain class.
  '.cm-panel.cm-search': {
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: '6px 8px',
    padding: '8px calc(40px + var(--source-search-inset, 0px)) 8px 8px',
    fontSize: '12px',
  },
  '.cm-panel.cm-search input, .cm-panel.cm-search button, .cm-panel.cm-search label': {
    margin: 0,
  },
  // Set apart from each other and from the buttons before them.
  '.cm-panel.cm-search [name=prev], .cm-panel.cm-search label': {
    marginRight: '4px',
  },
  '.cm-search-count': {
    minWidth: '3.5em',
    color: 'var(--whui-text-secondary)',
    fontVariantNumeric: 'tabular-nums',
    textAlign: 'center',
    whiteSpace: 'nowrap',
  },
  '.cm-search-count:empty': {
    display: 'none',
  },
  // Dimmed only once a recount runs long enough to be seen.
  '.cm-search-count-stale': {
    opacity: 0.5,
    transition: 'opacity 0s 150ms',
  },
  '.cm-panel.cm-search label': {
    display: 'inline-flex',
    alignItems: 'center',
    gap: '6px',
    fontSize: 'inherit',
    cursor: 'pointer',
  },
  // antd's checkbox in place of the native one.
  '.cm-panel.cm-search input[type=checkbox]': {
    appearance: 'none',
    position: 'relative',
    flex: 'none',
    boxSizing: 'border-box',
    width: '14px',
    height: '14px',
    margin: 0,
    border: '1px solid var(--whui-border-strong)',
    borderRadius: 'var(--whui-radius-sm)',
    backgroundColor: 'transparent',
    cursor: 'inherit',
  },
  '.cm-panel.cm-search label:hover input[type=checkbox]': {
    borderColor: 'var(--whui-primary)',
  },
  '.cm-panel.cm-search input[type=checkbox]:checked': {
    backgroundColor: 'var(--whui-primary)',
    borderColor: 'var(--whui-primary)',
  },
  '.cm-panel.cm-search input[type=checkbox]:checked::after': {
    content: '""',
    position: 'absolute',
    top: '50%',
    left: '21.5%',
    boxSizing: 'border-box',
    width: '5px',
    height: '8px',
    border: 'solid #fff',
    borderWidth: '0 2px 2px 0',
    transform: 'rotate(45deg) translate(-50%, -50%)',
  },
  '.cm-panel.cm-search [name=close]': {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    top: '8px',
    right: 'calc(8px + var(--source-search-inset, 0px))',
    width: '24px',
    height: '24px',
    borderRadius: 'var(--whui-radius-sm)',
    color: 'var(--whui-text-secondary)',
    fontSize: '14px',
    cursor: 'pointer',
  },
  '.cm-panel.cm-search [name=close]:hover': {
    color: 'var(--whui-editor-fg)',
    backgroundColor: 'var(--whui-hover-bg)',
  },
  '.cm-textfield, .cm-button': {
    boxSizing: 'border-box',
    height: '24px',
    fontSize: 'inherit',
    color: 'inherit',
    border: '1px solid var(--whui-border-strong)',
    borderRadius: 'var(--whui-radius-sm)',
  },
  '.cm-textfield': {
    padding: '0 7px',
    backgroundColor: 'var(--whui-background-color)',
  },
  '.cm-textfield:focus': {
    outline: 'none',
    borderColor: 'var(--whui-primary)',
  },
  '.cm-button': {
    padding: '0 8px',
    backgroundImage: 'none',
    backgroundColor: 'transparent',
  },
  '.cm-button:hover': {
    backgroundColor: 'var(--whui-hover-bg)',
  },
  '.cm-button:active': {
    backgroundImage: 'none',
  },

  // VS Code's find colors, the current match told apart per theme below.
  '.cm-searchMatch': {
    backgroundColor: 'rgba(234, 92, 0, 0.33)',
    outline: 'none',
  },
});

// Only base themes can scope a rule to dark or light, so what differs between
// the two comes as a theme of its own per side.
const darkAppearance: Extension = [
  EditorView.theme(
    { '.cm-searchMatch.cm-searchMatch-selected': { backgroundColor: '#515c6a' } },
    { dark: true }
  ),
  syntaxHighlighting(darkHighlightStyle),
];

const lightAppearance: Extension = [
  EditorView.theme({
    '.cm-searchMatch.cm-searchMatch-selected': { backgroundColor: '#a8ac94' },
  }),
  syntaxHighlighting(lightHighlightStyle),
];

export function sourceCodeAppearance(dark: boolean): Extension {
  return dark ? darkAppearance : lightAppearance;
}
