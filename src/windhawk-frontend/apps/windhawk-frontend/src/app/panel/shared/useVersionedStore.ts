import { useCallback, useLayoutEffect, useRef, useState } from 'react';

/**
 * State whose keys can be written locally while a read of the whole set is out.
 *
 * A read is applied only where the key has not moved since the read was asked
 * for, so a read never puts back a key this window has already changed - or
 * removed. That is the one question a request's own reply cannot answer: it says
 * which request an answer belongs to, not whether the answer was overtaken by
 * something else this window did while it was on the wire. Both hosts answer
 * requests concurrently, and reading a whole set outlasts a single command, so a
 * read can be taken off the machine before a command the host has since carried
 * out and answered for.
 */
export function useVersionedStore<T>() {
  // The whole set, replaced. Marks nothing, because what goes through here is
  // the set arriving in the first place or a change this window did not make,
  // and a read is not stale with respect to either.
  const [items, setItems] = useState<Record<string, T> | null>(null);

  // The set as it stands, for a caller that has to look at a key before it can
  // say what to do. Written in the commit rather than after it: a passive effect
  // is flushed in a task of its own, and a caller reading in between would be
  // answered against the set before last.
  const itemsRef = useRef(items);
  useLayoutEffect(() => {
    itemsRef.current = items;
  }, [items]);

  const held = useCallback((key: string) => itemsRef.current?.[key], []);

  // Every write this window has made, counted, and the count each key was last
  // written at.
  const writesRef = useRef(0);
  const writtenAtRef = useRef(new Map<string, number>());

  // Where the count stands, for a read to carry back with what it answers.
  const mark = useCallback(() => writesRef.current, []);

  // One key, as a write of this window's own leaves it: a command it sent and
  // the host answered, or the echo of something it wrote. `next` is handed what
  // is held for the key - nothing, where the set does not have it - and answers
  // with what to hold, or with undefined to hold nothing. An updater rather than
  // a value because two writes can land before either has been rendered, and the
  // second has to see the first.
  const applyWrite = useCallback(
    (key: string, next: (held: T | undefined) => T | undefined) => {
      writtenAtRef.current.set(key, ++writesRef.current);
      setItems((prev) => {
        // Nothing held is nothing to write into, and nothing to invent to write
        // it into: the read that fills the set is still on its way.
        if (!prev) {
          return prev;
        }
        const written = next(prev[key]);
        if (written === prev[key]) {
          return prev;
        }
        const applied = { ...prev };
        if (written === undefined) {
          delete applied[key];
        } else {
          applied[key] = written;
        }
        return applied;
      });
    },
    []
  );

  // The keys a read found, over the ones held here. `at` is what `mark` gave
  // when the read was asked for, so a key written since then keeps what this
  // window holds for it - the entry, or its absence where the key was removed -
  // rather than being put back the way a read taken before the write describes
  // it.
  const applyRead = useCallback((all: Record<string, T>, at: number) => {
    setItems((prev) => {
      // Nothing held is nothing a write landed on, so the read stands whole.
      if (!prev) {
        return all;
      }
      const applied = { ...all };
      for (const [key, writtenAt] of writtenAtRef.current) {
        if (writtenAt <= at) {
          continue;
        }
        const heldValue = prev[key];
        if (heldValue !== undefined) {
          applied[key] = heldValue;
        } else {
          delete applied[key];
        }
      }
      return applied;
    });
  }, []);

  return { items, setItems, held, mark, applyWrite, applyRead };
}
