import * as monaco from 'monaco-editor/esm/vs/editor/editor.api.js';
import { type ResolvedTheme } from './theme';

// Monaco themes are global (one active theme across every editor), so both the
// settings editor and the log pane share this one.
export const MONACO_APP_THEME = 'windhawk-app';

function cssColorToHex(color: string): string | null {
  const match = color.match(/^rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?/);
  if (!match) {
    return null;
  }
  // A transparent read - e.g. mid theme-switch, before the incoming stylesheet
  // has applied --whui-background-color - would otherwise map to #000000 and
  // paint the editor black. Treat it as unreadable and let the caller default.
  if (match[4] !== undefined && Number(match[4]) === 0) {
    return null;
  }
  const toHex = (n: string) => Number(n).toString(16).padStart(2, '0');
  return `#${toHex(match[1])}${toHex(match[2])}${toHex(match[3])}`;
}

// Define (or refresh) a Monaco theme whose background matches the app
// background - the same --whui-background-color the page body and the source
// view use - so editors blend in instead of showing Monaco's default
// white / near-black. The body carries that background, so read its resolved
// color; if it can't be read as an opaque color, fall back to the theme's base
// (these mirror @whui-bg-fallback in the LESS bundles).
export function applyMonacoAppTheme(themeKind: ResolvedTheme) {
  const background =
    cssColorToHex(getComputedStyle(document.body).backgroundColor) ??
    (themeKind === 'light' ? '#f5f5f5' : '#1e1e1e');
  monaco.editor.defineTheme(MONACO_APP_THEME, {
    base: themeKind === 'light' ? 'vs' : 'vs-dark',
    inherit: true,
    rules: [],
    colors: { 'editor.background': background },
  });
  monaco.editor.setTheme(MONACO_APP_THEME);
}
