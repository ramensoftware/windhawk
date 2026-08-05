import { useCallback, useState } from 'react';

/**
 * A flag kept across visits, held in state and written through to localStorage.
 *
 * Anything not stored as the string `true` reads as off, so a key that was never
 * written, or holds something else, starts at the same place a fresh install
 * does.
 *
 * Storage can be closed to a webview, and reading it there throws rather than
 * coming back empty. That is answered by leaving the flag at its default and
 * carrying on: the choice then lasts as long as the screen it was made on, which
 * is worth more than the render it would otherwise take down with it.
 */
export function usePersistedFlag(storageKey: string): [boolean, () => void] {
  const [value, setValue] = useState(() => {
    try {
      return localStorage.getItem(storageKey) === 'true';
    } catch {
      return false;
    }
  });

  const toggle = useCallback(() => {
    setValue((current) => {
      try {
        localStorage.setItem(storageKey, (!current).toString());
      } catch {
        // Ignore storage failures; the choice still holds for this screen.
      }
      return !current;
    });
  }, [storageKey]);

  return [value, toggle];
}

export default usePersistedFlag;
