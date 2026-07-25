import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react';

import type { AppTheme } from '@windhawk/webview-ipc-contract';

// AppTheme (the theme setting: an explicit choice, or 'auto' to follow the host's
// light/dark preference - the VSCode theme in the webview, the OS elsewhere) is part of
// the shared webview IPC contract; re-export it from the single source. 'auto' is
// resolved to a concrete `ResolvedTheme` at the point of use.
export type { AppTheme };

// The concrete theme actually applied, after resolving 'auto'.
export type ResolvedTheme = 'dark' | 'light';

const THEME_STORAGE_KEY = 'windhawk-theme';

declare const WEBPACK_IS_TAURI: boolean;
declare const WEBPACK_IS_VSCODE: boolean;

// In the VSCode webview the app follows the host theme out of the box, so 'auto'
// is the default there; elsewhere it is the dark theme.
const DEFAULT_THEME: AppTheme = WEBPACK_IS_VSCODE ? 'auto' : 'dark';

declare global {
  interface Window {
    // Set by the Tauri native shell's init script (the already-resolved,
    // registry-backed theme) before the bundle runs, so the pre-render apply below
    // avoids a flash. Absent on the VSCode/website builds, which use localStorage.
    __WH_INITIAL_THEME__?: string;
  }
}

function isAppTheme(value: unknown): value is AppTheme {
  return value === 'dark' || value === 'light' || value === 'auto';
}

function mediaPrefersDark(): boolean {
  return (
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-color-scheme: dark)').matches
  );
}

// Whether the host currently prefers a dark color scheme, the value 'auto' resolves
// against. In the VSCode webview the host theme is exposed as a class on <body>
// (vscode-dark / vscode-light, plus the high-contrast variants), so read that; if VSCode
// has not applied it yet, fall back to the media query. In the Tauri shell the webview's
// prefers-color-scheme follows the OS (the native side sets the WebView2 color scheme to
// auto), and the website runs in a normal browser, so both use the media query.
function hostPrefersDark(): boolean {
  if (WEBPACK_IS_VSCODE) {
    const classes = document.body.classList;
    if (
      classes.contains('vscode-light') ||
      classes.contains('vscode-high-contrast-light')
    ) {
      return false;
    }
    if (
      classes.contains('vscode-dark') ||
      classes.contains('vscode-high-contrast')
    ) {
      return true;
    }
  }
  return mediaPrefersDark();
}

// Subscribe to host light/dark preference changes while a setting of 'auto' is in effect,
// returning an unsubscribe callback. In VSCode the preference changes via the <body> theme
// class (VSCode rewrites it when the user switches themes), so observe that; elsewhere it
// changes via the prefers-color-scheme media query.
function subscribeToHostThemeChanges(onChange: () => void): () => void {
  if (WEBPACK_IS_VSCODE) {
    const observer = new MutationObserver(onChange);
    observer.observe(document.body, {
      attributes: true,
      attributeFilter: ['class'],
    });
    return () => observer.disconnect();
  }
  const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
  mediaQuery.addEventListener('change', onChange);
  return () => mediaQuery.removeEventListener('change', onChange);
}

// Resolve a setting to the concrete theme to render: 'auto' follows the host, the rest map
// to themselves.
export function resolveTheme(theme: AppTheme): ResolvedTheme {
  if (theme === 'auto') {
    return hostPrefersDark() ? 'dark' : 'light';
  }
  return theme;
}

// The theme to apply before the first render. In Tauri it comes from the global the
// native shell injects (already resolved from the registry-backed setting; the
// authoritative setting still arrives over IPC and the provider is controlled by it, so
// this is only the first-paint hint). Everywhere else it is the localStorage-persisted
// choice.
export function readStoredTheme(): AppTheme {
  if (WEBPACK_IS_TAURI) {
    const injected = window.__WH_INITIAL_THEME__;
    return isAppTheme(injected) ? injected : DEFAULT_THEME;
  }
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    return isAppTheme(stored) ? stored : DEFAULT_THEME;
  } catch {
    return DEFAULT_THEME;
  }
}

// Activate the matching antd stylesheet (the two `<link data-theme-stylesheet>`
// tags live in index.html) and reflect the active theme on `<body data-theme>`,
// which is what the CSS token layer and styled-components resolve their
// light/dark values against. Kept as a plain side effect so it can run before
// React mounts, avoiding a flash of the wrong theme.
//
// The swap toggles `media`, not `disabled`: both bundles are loaded up front
// (the inactive one via media="not all", which loads in the background without
// blocking render), so switching is an instant swap between two already-parsed
// stylesheets. Toggling `disabled` instead defers loading the incoming bundle
// until the first switch, which flashes unstyled content and, mid-reflow,
// breaks the layout of any open antd popup.
export function applyThemeToDocument(theme: AppTheme) {
  const resolved = resolveTheme(theme);
  // Skip the DOM writes when the resolved theme is already applied. In the VSCode
  // webview `apply` is driven by a MutationObserver on the <body> class, which VSCode
  // rewrites for many reasons that don't change the resolved theme; `data-theme` is a
  // reliable "already applied" signal since this is the sole writer of it and the links.
  if (document.body.dataset['theme'] === resolved) {
    return;
  }
  const links = document.querySelectorAll<HTMLLinkElement>(
    'link[data-theme-stylesheet]'
  );
  links.forEach((link) => {
    link.media =
      link.dataset['themeStylesheet'] === resolved ? 'all' : 'not all';
  });
  document.body.dataset['theme'] = resolved;
}

type ThemeContextValue = {
  // The setting (may be 'auto'); use this for the theme picker.
  theme: AppTheme;
  // The concrete theme in effect (never 'auto'); use this to render theme-dependent
  // colors (e.g. Monaco). It tracks the host preference live while `theme` is 'auto'.
  resolvedTheme: ResolvedTheme;
  setTheme: (theme: AppTheme) => void;
};

const ThemeContext = createContext<ThemeContextValue>({
  theme: DEFAULT_THEME,
  resolvedTheme: 'dark',
  setTheme: () => undefined,
});

type ThemeProviderProps = React.PropsWithChildren<{
  // Tauri: the backend-owned theme (from appUISettings) and the callback that persists a
  // change to the registry via updateAppSettings. When onPersistTheme is set the provider
  // is CONTROLLED by backendTheme and never touches localStorage - a change is written to
  // the registry, echoed back over setNewAppSettings as a new backendTheme, and applied
  // from there (the same round-trip the language setting uses). When absent (VSCode,
  // website) the theme is localStorage-backed and applied optimistically.
  backendTheme?: AppTheme;
  onPersistTheme?: (theme: AppTheme) => void;
}>;

export function ThemeProvider({
  backendTheme,
  onPersistTheme,
  children,
}: ThemeProviderProps) {
  const controlled = onPersistTheme !== undefined;
  const [localTheme, setLocalTheme] = useState<AppTheme>(readStoredTheme);
  const theme = controlled ? backendTheme ?? DEFAULT_THEME : localTheme;

  const [resolvedTheme, setResolvedTheme] = useState<ResolvedTheme>(() =>
    resolveTheme(theme)
  );

  // Apply the resolved theme whenever the setting changes, and - while it is 'auto' -
  // whenever the host light/dark preference changes, so the app follows the host live.
  useEffect(() => {
    const apply = () => {
      applyThemeToDocument(theme);
      setResolvedTheme(resolveTheme(theme));
    };
    apply();
    if (theme !== 'auto') {
      return;
    }
    return subscribeToHostThemeChanges(apply);
  }, [theme]);

  const setTheme = useCallback(
    (next: AppTheme) => {
      if (onPersistTheme) {
        // Controlled: persist to the registry; the DOM apply follows when the backend
        // echoes the new setting back (backendTheme -> the effect above).
        onPersistTheme(next);
        return;
      }
      try {
        localStorage.setItem(THEME_STORAGE_KEY, next);
      } catch {
        // Ignore storage failures (e.g. restricted webview storage); the choice
        // still applies for the current session.
      }
      // Apply to the DOM synchronously so consumers that read the resolved
      // background on the same tick (e.g. the Monaco theme) see the new state.
      applyThemeToDocument(next);
      setResolvedTheme(resolveTheme(next));
      setLocalTheme(next);
    },
    [onPersistTheme]
  );

  const value = useMemo(
    () => ({ theme, resolvedTheme, setTheme }),
    [theme, resolvedTheme, setTheme]
  );

  return (
    <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
  );
}

export function useTheme() {
  return useContext(ThemeContext);
}
