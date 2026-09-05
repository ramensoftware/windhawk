// Two of Monaco's internals, neither of which ships types. Its own default
// colour provider is built from exactly these, and monacoArgbColors.ts reuses
// them so that everything but the position of the alpha byte in a hex colour
// stays Monaco's own behaviour. Declared here for the parts that are used.

declare module 'monaco-editor/editor/common/languages/defaultDocumentColorsComputer.js' {
  import type { languages } from 'monaco-editor/editor/editor.api.js';

  // Duck-typed by the computer against the editor worker's mirror model.
  export interface DocumentColorsModel {
    getValue(): string;
    positionAt(offset: number): { lineNumber: number; column: number };
    findMatches(regex: RegExp): RegExpMatchArray[];
  }

  export function computeDefaultDocumentColors(
    model: DocumentColorsModel
  ): languages.IColorInformation[];
}

declare module 'monaco-editor/base/common/color.js' {
  export class RGBA {
    constructor(r: number, g: number, b: number, a?: number);
  }

  export class Color {
    constructor(rgba: RGBA);
    static Format: {
      CSS: {
        formatRGB(color: Color): string;
        formatHSL(color: Color): string;
      };
    };
  }
}
