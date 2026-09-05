import { Color, RGBA } from 'monaco-editor/base/common/color.js';
import { computeDefaultDocumentColors } from 'monaco-editor/editor/common/languages/defaultDocumentColorsComputer.js';
import * as monaco from 'monaco-editor/editor/editor.api.js';

// Mod settings name colours the way XAML does, alpha first: #AARRGGBB. Monaco
// reads eight hex digits as #RRGGBBAA, so its swatches and its colour picker
// disagree with every mod that takes a colour.
//
// A colour provider registered for a language displaces Monaco's built-in one,
// which is registered for '*' and only consulted when nothing else answers, so
// this provider is free to be Monaco's behaviour with one thing changed: its
// matcher finds the colours, and only the hex readings are re-interpreted.
// rgb() and hsl() pass through untouched.

const HEX_WITH_ALPHA = /^#(?:[0-9A-Fa-f]{4}|[0-9A-Fa-f]{8})$/;

function parseArgbHex(hex: string): monaco.languages.IColor {
  const digits = hex.slice(1);
  // #ARGB is #AARRGGBB with every digit doubled.
  const expanded = digits.length === 4 ? digits.replace(/./g, (d) => d + d) : digits;
  const channel = (index: number) =>
    parseInt(expanded.slice(index * 2, index * 2 + 2), 16) / 255;
  return {
    alpha: channel(0),
    red: channel(1),
    green: channel(2),
    blue: channel(3),
  };
}

function formatArgbHex({ red, green, blue, alpha }: monaco.languages.IColor) {
  const channel = (value: number) =>
    Math.round(value * 255)
      .toString(16)
      .padStart(2, '0');
  const rgb = `${channel(red)}${channel(green)}${channel(blue)}`;
  // An opaque colour needs no alpha byte, as in Monaco's own hex presentation.
  return alpha === 1 ? `#${rgb}` : `#${channel(alpha)}${rgb}`;
}

const argbColorProvider: monaco.languages.DocumentColorProvider = {
  provideDocumentColors(model) {
    const text = model.getValue();
    const found = computeDefaultDocumentColors({
      getValue: () => text,
      positionAt: (offset) => model.getPositionAt(offset),
      findMatches: (regex) => Array.from(text.matchAll(regex)),
    });
    return found.map((color) => {
      const matched = model.getValueInRange(color.range);
      return HEX_WITH_ALPHA.test(matched)
        ? { ...color, color: parseArgbHex(matched) }
        : color;
    });
  },

  provideColorPresentations(model, { color, range }) {
    const value = new Color(
      new RGBA(
        Math.round(255 * color.red),
        Math.round(255 * color.green),
        Math.round(255 * color.blue),
        color.alpha
      )
    );
    return [
      Color.Format.CSS.formatRGB(value),
      Color.Format.CSS.formatHSL(value),
      formatArgbHex(color),
    ].map((text) => ({ label: text, textEdit: { range, text } }));
  },
};

let registered = false;

// Monaco's language registrations are global, as its themes are. A second one
// would answer alongside the first and stack a duplicate swatch on every colour.
export function registerMonacoArgbColors() {
  if (registered) {
    return;
  }
  registered = true;
  monaco.languages.registerColorProvider('yaml', argbColorProvider);
}
