import { type ConfigProviderProps, theme, type ThemeConfig } from 'antd';
import type { CSSProperties } from 'react';

import type { ResolvedTheme } from './theme';

// antd's theme for each of the app's themes, and the --whui-* variables the
// app's own styles read. The variables that name an antd color are read off the
// same tokens antd draws with, so the two cannot drift apart.

const ALGORITHM = {
  dark: theme.darkAlgorithm,
  light: theme.defaultAlgorithm,
};

// Deeper than antd's description gray, which washes out on the white card.
const LIGHT_TEXT_MUTED = 'rgba(0, 0, 0, 0.6)';

// Deeper than antd's #fadb14, so the stars do not wash out on the white card.
const LIGHT_RATE_STAR = '#e6c200';

function buildAntdTheme(resolved: ResolvedTheme): ThemeConfig {
  const algorithm = ALGORITHM[resolved];
  const base = theme.getDesignToken({ algorithm });

  // The app's standard outlined button is a ghost one, on whichever surface
  // holds it. antd means ghost for a colored background, and draws it in the
  // container color, which on the app's surfaces is no color at all.
  const Button = {
    defaultGhostColor: base.colorText,
    defaultGhostBorderColor: base.colorBorder,
  };

  // antd draws a slider's track and handle in the primary's pale border tint
  // until it is hovered, which reads as disabled beside a checked switch or
  // checkbox. They take the primary and its hover color, as those do.
  const Slider = {
    trackBg: base.colorPrimary,
    trackHoverBg: base.colorPrimaryHover,
    handleColor: base.colorPrimary,
    dotActiveBorderColor: base.colorPrimary,
    // The handle's ring while the slider is hovered, which antd reads off this
    // global token rather than a slider one.
    colorPrimaryBorderHover: base.colorPrimaryHover,
  };

  if (resolved === 'dark') {
    // A tooltip, a popover and the menus and lists that drop down from a
    // control are one floating surface: the tooltip's here, which stands out
    // from the page and the card where antd's elevated surface nearly matches
    // them.
    const floating = { colorBgElevated: base.colorBgSpotlight };
    return {
      algorithm,
      components: {
        Button,
        Slider,
        Popover: { ...floating, colorText: base.colorTextLightSolid },
        Dropdown: floating,
        Select: floating,
      },
    };
  }

  return {
    algorithm,
    token: {
      // antd's secondary border (#f0f0f0) barely separates the app's stacked
      // surfaces, so every light-theme border reads at the base weight.
      colorBorderSecondary: base.colorBorder,
    },
    components: {
      Button,
      Slider,
      // One hover surface, the popover's here: white, with the on-surface text.
      Tooltip: {
        colorBgSpotlight: base.colorBgElevated,
        colorTextLightSolid: base.colorText,
      },
      // An empty star in antd's fill rather than its faintest one, which the
      // white card all but swallows: it stands out as much as a dark theme's
      // empty star does from its card.
      Rate: { starColor: LIGHT_RATE_STAR, starBg: base.colorFill },
      // A warning tag in deeper golds than antd's, whose text is faint on its
      // fill and whose fill the white card all but swallows: the text reads at
      // body contrast, and the fill stands out as much as a dark theme's does.
      Tag: { colorWarning: base.gold9, colorWarningBg: base.gold3 },
      // antd tracks a segmented control in the page color, which leaves one
      // standing on the page with no track. A fill comes out the same on a card
      // and still shows on the page.
      Segmented: { trackBg: base.colorFillTertiary },
      Card: { colorTextDescription: LIGHT_TEXT_MUTED },
      Listy: { colorSplit: base.colorBorder },
    },
  };
}

const ANTD_THEME: Record<ResolvedTheme, ThemeConfig> = {
  dark: buildAntdTheme('dark'),
  light: buildAntdTheme('light'),
};

export function antdTheme(resolved: ResolvedTheme): ThemeConfig {
  return ANTD_THEME[resolved];
}

// antd pads a dialog's container around its body, so a body the app scrolls or
// clips would cut a focused control's ring off at its edge and scroll inside the
// padding. The body reaches across the inline padding and takes it as its own.
const MODAL_TOKENS = theme.getDesignToken();
const MODAL_INSET = MODAL_TOKENS.paddingContentHorizontalLG;

export const ANTD_MODAL_CONFIG: ConfigProviderProps['modal'] = {
  styles: {
    body: { marginInline: -MODAL_INSET, paddingInline: MODAL_INSET },
  },
};

// The same across the bottom padding, for a dialog with no footer: a body that
// scrolls runs to the dialog's edge, and the dialog clips it, scrollbar
// included, to its rounded corners.
export const MODAL_FOOTERLESS_STYLES: Record<'container' | 'body', CSSProperties> = {
  container: { overflow: 'hidden' },
  body: {
    marginBottom: -MODAL_TOKENS.paddingMD,
    paddingBottom: MODAL_TOKENS.paddingMD,
  },
};

function withAlpha(hexColor: string, alpha: number) {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hexColor.slice(i, i + 2), 16));
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

// The app's own colors, which antd has no token for.
const APP_COLORS: Record<ResolvedTheme, Record<string, string>> = {
  dark: {
    // The panel follows the webview's own theme, not a host editor color.
    '--whui-background-color': '#1e1e1e',
    '--whui-logo-color': '#ffffff',
    '--whui-logo-color-hover': '#ffffff',
    '--whui-text-muted': 'rgba(255, 255, 255, 0.45)',
    '--whui-preformat': '#d7ba7d',
    '--whui-link': '#388ed3',
    '--whui-link-hover': '#4daafc',
    // Reads as "pending" next to the primary blue of the non-default mark
    // without competing with it.
    '--whui-setting-unsaved': '#d89614',
    '--whui-border-faint': 'rgba(255, 255, 255, 0.2)',
    '--whui-divider': '#454545',
    // Translucent, so a chip lifts off whichever surface holds it: the panel,
    // the modal, and the lighter popover alike.
    '--whui-chip-bg': 'rgba(255, 255, 255, 0.17)',
    // Lighter than both the panel and the card, so a control floating above the
    // form reads as raised over either.
    '--whui-float-bg': '#2d2d2d',
    '--whui-inline-code-bg': 'rgba(60, 60, 60, 0.4)',
    '--whui-hover-bg': 'rgba(127, 127, 127, 0.22)',
    '--whui-track-bg': 'rgba(23, 18, 18, 0.1)',
    '--whui-skeleton-base': 'rgba(255, 255, 255, 0.08)',
    '--whui-editor-fg': '#d4d4d4',
    '--whui-diff-gutter-insert': '#082525',
    '--whui-diff-gutter-delete': '#2b1523',
    '--whui-diff-code-insert': '#082525',
    '--whui-diff-code-delete': '#2b1523',
    '--whui-diff-code-insert-edit': '#00462f',
    '--whui-diff-code-delete-edit': '#4e2436',
    '--whui-diff-omit': 'rgba(255, 255, 255, 0.04)',
  },
  light: {
    // A shade below the white card, so the two layers read distinctly.
    '--whui-background-color': '#f5f5f5',
    '--whui-logo-color': '#000000d9',
    '--whui-logo-color-hover': '#000000d9',
    '--whui-text-muted': LIGHT_TEXT_MUTED,
    '--whui-preformat': '#a31515',
    '--whui-link': '#1677ff',
    '--whui-link-hover': '#4096ff',
    // antd's warning, deepened a shade so a 3px bar of it still reads against
    // the white card.
    '--whui-setting-unsaved': '#d48806',
    '--whui-border-faint': 'rgba(0, 0, 0, 0.12)',
    '--whui-divider': '#e8e8e8',
    '--whui-chip-bg': 'rgba(0, 0, 0, 0.06)',
    // The card is already white, so what raises a floating control here is its
    // border and the shadow under it rather than its fill.
    '--whui-float-bg': '#ffffff',
    '--whui-inline-code-bg': 'rgba(0, 0, 0, 0.06)',
    '--whui-hover-bg': 'rgba(0, 0, 0, 0.06)',
    '--whui-track-bg': 'rgba(0, 0, 0, 0.06)',
    '--whui-skeleton-base': 'rgba(0, 0, 0, 0.06)',
    '--whui-editor-fg': '#333333',
    // GitHub's palette.
    '--whui-diff-gutter-insert': '#cdffd8',
    '--whui-diff-gutter-delete': '#ffdce0',
    '--whui-diff-code-insert': '#e6ffed',
    '--whui-diff-code-delete': '#ffeef0',
    '--whui-diff-code-insert-edit': '#acf2bd',
    '--whui-diff-code-delete-edit': '#fdb8c0',
    '--whui-diff-omit': 'rgba(0, 0, 0, 0.03)',
  },
};

// Used by the sidebar where it renders outside a real VSCode webview, which
// otherwise supplies --vscode-sideBar-background.
const SIDEBAR_BACKGROUND_FALLBACK: Record<ResolvedTheme, string> = {
  dark: '#252526',
  light: '#ececec',
};

export function themeVariables(resolved: ResolvedTheme): Record<string, string> {
  const token = theme.getDesignToken(ANTD_THEME[resolved]);
  return {
    '--whui-text': token.colorText,
    '--whui-text-secondary': token.colorTextSecondary,
    '--whui-font-family': token.fontFamily,
    '--whui-card-background-color': token.colorBgContainer,
    // The surface of a dialog, for what is drawn over its scrolled body.
    '--whui-modal-background-color': token.colorBgElevated,
    '--whui-primary': token.colorPrimary,
    // How antd draws a focused field, for a control beside one that has to read
    // as focused the same way.
    '--whui-focus-border': token.colorPrimary,
    '--whui-focus-ring': token.controlOutline,
    '--whui-border': token.colorBorderSecondary,
    '--whui-border-strong': token.colorBorder,
    // antd's corners: a chip or an item inside a control, a control, and a
    // container such as a card.
    '--whui-radius-sm': `${token.borderRadiusSM}px`,
    '--whui-radius': `${token.borderRadius}px`,
    '--whui-radius-lg': `${token.borderRadiusLG}px`,
    // The edge of a bar pinned over content that scrolls under it: antd's
    // shadow for a borderless card.
    '--whui-stuck-shadow': token.boxShadowTertiary,
    '--whui-rate-star':
      resolved === 'light' ? LIGHT_RATE_STAR : token.yellow6,
    // The links on the dark theme's tooltip surface, a gray antd's link blue
    // all but disappears into: two steps lighter in the same palette.
    ...(resolved === 'dark' && {
      '--whui-floating-link': token.blue9,
      '--whui-floating-link-hover': token.blue10,
    }),
    // The two states a mod settings row is marked with, in the gutter beside
    // it: holding something other than the value its mod declares, and holding
    // an edit that is not saved yet.
    '--whui-setting-non-default': token.colorPrimary,
    // The tint a settings row is washed with for a moment after it is carried
    // to another place in its array.
    '--whui-setting-moved-bg': withAlpha(token.colorPrimary, 0.2),
    // The tint of a selected mod, in either view of the installed mods. Derived
    // from the primary so "selected" is one color in the app.
    '--whui-selected-bg': withAlpha(token.colorPrimary, 0.15),
    ...APP_COLORS[resolved],
  };
}

const VARIABLES_STYLE_ID = 'whui-theme-variables';

function themeRules(resolved: ResolvedTheme) {
  const body = `body[data-theme="${resolved}"]`;
  const declarations = Object.entries(themeVariables(resolved))
    .map(([name, value]) => `${name}: ${value};`)
    .join(' ');
  return (
    `${body} { ${declarations} }\n` +
    `${body}[data-content="sidebar"] { --whui-background-color: ` +
    `var(--vscode-sideBar-background, ${SIDEBAR_BACKGROUND_FALLBACK[resolved]}); }`
  );
}

// Puts the variables of both themes in the document, keyed by
// `body[data-theme]`, so switching theme is a matter of that attribute alone.
export function installThemeVariables() {
  if (document.getElementById(VARIABLES_STYLE_ID)) {
    return;
  }
  const style = document.createElement('style');
  style.id = VARIABLES_STYLE_ID;
  style.textContent = `${themeRules('dark')}\n${themeRules('light')}`;
  document.head.appendChild(style);
}
