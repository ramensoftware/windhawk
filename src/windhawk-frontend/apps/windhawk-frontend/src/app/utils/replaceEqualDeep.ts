/**
 * `next`, with every part of it that is deeply equal to the same part of `prev`
 * taken from `prev`: `prev` itself where the two are equal throughout, and
 * otherwise a copy of `next` that shares each unchanged sub-tree with `prev`.
 * What is memoized over the value, or drawn from it, then sees a new object
 * only where something moved.
 *
 * For JSON-like values: plain objects, arrays and primitives. Anything else is
 * equal only to itself.
 */
export function replaceEqualDeep<T>(prev: unknown, next: T): T {
  if (prev === next) {
    return next;
  }

  if (Array.isArray(prev) && Array.isArray(next)) {
    const copy = next.map((item, index) => replaceEqualDeep(prev[index], item));
    const kept =
      copy.length === prev.length &&
      copy.every((item, index) => item === prev[index]);
    return (kept ? prev : copy) as T;
  }

  if (isPlainObject(prev) && isPlainObject(next)) {
    const copy: Record<string, unknown> = {};
    for (const key of Object.keys(next)) {
      copy[key] = replaceEqualDeep(prev[key], next[key]);
    }
    const keys = Object.keys(copy);
    // A key held as undefined is not the key left out.
    const kept =
      keys.length === Object.keys(prev).length &&
      keys.every(
        (key) =>
          copy[key] === prev[key] &&
          Object.prototype.hasOwnProperty.call(prev, key)
      );
    return (kept ? prev : copy) as T;
  }

  return next;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const proto = Object.getPrototypeOf(value);
  return proto === Object.prototype || proto === null;
}
