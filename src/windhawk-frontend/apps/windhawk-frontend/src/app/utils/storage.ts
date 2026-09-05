/**
 * Best-effort access to the localStorage keys that hold a single string.
 *
 * Storage can be closed to the app - a browser told to block site data, a webview
 * with storage restricted - and a write can be refused for want of room, both of
 * which throw rather than answering empty.
 * Both sides swallow that: a read reports nothing stored, and a write leaves the
 * value holding for as long as the screen is open. What is kept this way is a
 * preference the app runs fine without, so a caller has nothing to handle and the
 * user has nothing to act on.
 *
 * A caller that stores a structure serializes it here and keeps its own parsing:
 * text that does not parse is its own answer to give, and unreadable storage
 * arrives as the same null an unwritten key does.
 */

export function readStoredValue(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeStoredValue(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Swallowed; see above.
  }
}
