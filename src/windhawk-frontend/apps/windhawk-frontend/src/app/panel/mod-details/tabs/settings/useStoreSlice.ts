import { useMemo, useSyncExternalStore } from 'react';

/**
 * A value held outside React, and the components that read it: a new value is
 * announced to each of them, and each is drawn again only if the part of it
 * that component reads has changed (see useStoreSlice).
 */
export type ValueStore<T> = {
  get: () => T;
  set: (value: T) => void;
  subscribe: (listener: () => void) => () => void;
};

export function createValueStore<T>(initial: T): ValueStore<T> {
  let value = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    set: (next) => {
      if (next === value) {
        return;
      }
      value = next;
      for (const listener of listeners) {
        listener();
      }
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/**
 * What `select` picks of the store's value. The part picked before is kept for
 * as long as `isEqual` finds the new one the same, so a value that changed
 * somewhere else does not draw the component again. A new `select` or `isEqual`
 * picks afresh, so both are expected to keep their identity across renders.
 */
export function useStoreSlice<T, Slice>(
  store: ValueStore<T>,
  select: (value: T) => Slice,
  isEqual: (a: Slice, b: Slice) => boolean
): Slice {
  const getSnapshot = useMemo(
    () => sliceReader(store, select, isEqual),
    [store, select, isEqual]
  );
  return useSyncExternalStore(store.subscribe, getSnapshot);
}

// Picks `select` of the store's value once a value, answering with the part
// picked before while the new one is equal to it.
function sliceReader<T, Slice>(
  store: ValueStore<T>,
  select: (value: T) => Slice,
  isEqual: (a: Slice, b: Slice) => boolean
): () => Slice {
  let read: { value: T; slice: Slice } | undefined;
  return () => {
    const value = store.get();
    if (read && read.value === value) {
      return read.slice;
    }
    const slice = select(value);
    read = { value, slice: read && isEqual(read.slice, slice) ? read.slice : slice };
    return read.slice;
  };
}
