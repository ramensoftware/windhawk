/**
 * A date as the day, its month in short, and the year, in the order and the
 * month names of the given language: "Sep 13, 2026" in English, "13 wrz 2026"
 * in Polish.
 */
export function formatShortDate(date: Date, language: string): string {
  return date.toLocaleDateString(language, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}
