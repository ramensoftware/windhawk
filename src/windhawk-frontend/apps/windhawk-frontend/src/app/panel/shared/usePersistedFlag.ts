import { readStoredValue, writeStoredValue } from '@app/utils';
import { useCallback, useState } from 'react';

/**
 * A flag kept across visits, held in state and written through to localStorage.
 *
 * Anything not stored as the string `true` reads as off, so a key that was never
 * written, or holds something else, starts at the same place a fresh install
 * does.
 */
export function usePersistedFlag(storageKey: string): [boolean, () => void] {
  const [value, setValue] = useState(
    () => readStoredValue(storageKey) === 'true'
  );

  const toggle = useCallback(() => {
    setValue((current) => {
      writeStoredValue(storageKey, (!current).toString());
      return !current;
    });
  }, [storageKey]);

  return [value, toggle];
}

export default usePersistedFlag;
