/**
 * The text a color setting holds, and the hex the picker speaks.
 *
 * A `$format: colorRgb` setting stores `#RRGGBB` and a `colorArgb` one
 * `#AARRGGBB` - alpha first, the layout the mods reading a color already parse.
 * The picker (react-colorful) speaks CSS hex instead: `#rrggbb` and
 * `#rrggbbaa`, alpha last. These two functions are the whole of the
 * conversion, so the control on either side of them deals in one spelling.
 *
 * Reading is lenient and writing strict, on purpose: a value a user typed or a
 * mod shipped without the `#` or in lowercase still draws its swatch, and the
 * picker writes uppercase with the `#` - the layout above - so a mod adopting
 * a color format has one spelling to parse. Pure: no React or DOM.
 */

export type ColorFormat = 'colorRgb' | 'colorArgb';

const COLOR_DIGITS: Record<ColorFormat, number> = {
  colorRgb: 6,
  colorArgb: 8,
};

/**
 * Whether a `$format` names one of the color formats this front-end draws.
 */
export function isColorFormat(format: string | undefined): format is ColorFormat {
  return format === 'colorRgb' || format === 'colorArgb';
}

/**
 * The picker's hex for a setting's text, or null for text that spells no color
 * in the declared layout - an empty string, a keyword, the wrong digit count -
 * which the swatch draws as no color and the text field leaves as it is.
 */
export function colorTextToPicker(text: string, format: ColorFormat): string | null {
  const digits = text.trim().replace(/^#/, '');
  if (digits.length !== COLOR_DIGITS[format] || !/^[0-9a-fA-F]+$/.test(digits)) {
    return null;
  }

  const hex = digits.toLowerCase();
  // AARRGGBB -> rrggbbaa.
  return '#' + (format === 'colorArgb' ? hex.slice(2) + hex.slice(0, 2) : hex);
}

/**
 * The text to store for a hex the picker produced: uppercase, with a `#`, in
 * the declared layout. The picker writes eight digits only for a translucent
 * color - an opaque one comes back as six even from the alpha picker - so an
 * absent alpha is the opaque one it stands for, and one the layout has no room
 * for is dropped: the stored text is always the declared width.
 */
export function pickerToColorText(hex: string, format: ColorFormat): string {
  const digits = hex.replace(/^#/, '').toUpperCase();
  const rgb = digits.slice(0, 6);
  if (format === 'colorRgb') {
    return '#' + rgb;
  }
  const alpha = digits.length >= 8 ? digits.slice(6, 8) : 'FF';
  return '#' + alpha + rgb;
}
