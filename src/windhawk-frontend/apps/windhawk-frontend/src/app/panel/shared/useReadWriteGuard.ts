import { useCallback, useRef } from 'react';

/**
 * Keeps a read of a single value from undoing a write this window made to it.
 *
 * A read is applied only where no write has been sent or answered since it was
 * asked for, and none is out when it lands. A write sent while a read is out
 * may or may not be in what the read took off the machine - both hosts answer
 * requests concurrently - and the write's own answer already says what the
 * value is; a write still out when the read lands leaves the same question open.
 *
 * Holds no value: what a read or a write leaves is the caller's to apply. The
 * keyed counterpart, for a set written one key at a time, is useVersionedStore.
 */
export function useReadWriteGuard() {
  // Every write sent and every write answered, counted, and the writes sent and
  // not answered yet.
  const movesRef = useRef(0);
  const openRef = useRef(0);

  // Where the count stands, for a read to carry back with what it answers.
  const mark = useCallback(() => movesRef.current, []);

  // Sends a write through `send` and resolves with what it resolves with,
  // counted as it goes out and as it settles, however it settles.
  const write = useCallback(async <R>(send: () => Promise<R>): Promise<R> => {
    movesRef.current++;
    openRef.current++;
    try {
      return await send();
    } finally {
      openRef.current--;
      movesRef.current++;
    }
  }, []);

  // Whether a read asked for at the mark `at` has been overtaken by a write,
  // and so is not to be applied.
  const overtaken = useCallback(
    (at: number) => movesRef.current !== at || openRef.current > 0,
    []
  );

  return { mark, write, overtaken };
}
