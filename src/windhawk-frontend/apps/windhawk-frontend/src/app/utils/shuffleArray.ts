/**
 * Shuffles array in place using Fisher-Yates algorithm.
 * https://stackoverflow.com/a/6274381
 */
export function shuffleArray<T>(a: T[]): T[] {
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}
