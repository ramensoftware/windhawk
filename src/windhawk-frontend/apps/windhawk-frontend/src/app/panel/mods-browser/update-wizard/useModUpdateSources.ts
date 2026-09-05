import { useGetModSourceData, useGetRepositoryModSourceData } from '@app/webviewIPC';
import { useCallback, useEffect, useRef, useState } from 'react';

/**
 * The sources a batch update needs, one entry per mod.
 *
 * `installMod` takes the source text rather than a mod id, so the repository
 * source has to be in hand before a mod can be updated at all - which is why it is
 * fetched up front for the whole list, and why the target version shown in a row
 * and the "new" side of its diff cost nothing extra: they are read off a fetch the
 * update needs regardless.
 *
 * The installed source is a second round trip and only the diff wants it, so it is
 * fetched per mod when that diff is first opened.
 */

export type ModUpdateSourceStatus = 'loading' | 'ready' | 'failed';

export type ModUpdateSource = {
  status: ModUpdateSourceStatus;
  // The repository's current source and the version it belongs to: what installMod
  // is given, what the row names as the target, and the 'new' side of the diff.
  source?: string;
  version?: string;
  // The installed source, absent until a diff asks for it.
  installedStatus?: ModUpdateSourceStatus;
  installedSource?: string;
};

/**
 * How many repository fetches are allowed to be open at once. A user with forty
 * updatable mods would otherwise open forty of them on a webview that also has a
 * list to render; four keeps the list filling visibly without a stall.
 */
const CONCURRENCY = 4;

/**
 * The repository reads one mount has going: the mods waiting for a slot, the mods
 * it has taken on, and how many workers are draining the queue.
 *
 * The three are held together because they end together. The IPC hooks abandon
 * every request they hold open when they unmount, so a mount's reads do not
 * outlive it; taking a fresh set is how the mount that follows starts its own
 * rather than inheriting a queue nobody is draining and a count of workers that
 * are not coming back.
 */
type SourceReads = {
  queue: string[];
  // The mods with a read queued or in flight. `loading` alone cannot say that: it
  // is also what a mod is left in when the mount that asked for it went away, and
  // telling those apart is what the fill effect asks.
  open: Set<string>;
  workers: number;
};

const newSourceReads = (): SourceReads => ({
  queue: [],
  open: new Set(),
  workers: 0,
});

export function useModUpdateSources(modIds: string[]) {
  const [sources, setSources] = useState<Record<string, ModUpdateSource>>({});

  // The state as the queue and the lazy fetches read it: they decide from it
  // within the same tick they write it, which a re-render would be too late for.
  const sourcesRef = useRef<Record<string, ModUpdateSource>>({});
  const writeSources = useCallback(
    (next: Record<string, ModUpdateSource>) => {
      sourcesRef.current = next;
      setSources(next);
    },
    []
  );
  const patchSource = useCallback(
    (modId: string, patch: Partial<ModUpdateSource>) => {
      const current = sourcesRef.current[modId];
      if (!current) {
        return;
      }
      writeSources({
        ...sourcesRef.current,
        [modId]: { ...current, ...patch },
      });
    },
    [writeSources]
  );

  const readsRef = useRef<SourceReads>(newSourceReads());

  const { getRepositoryModSourceData } = useGetRepositoryModSourceData();

  // Put as many workers on the queue as there are slots for. Each takes the next
  // mod, waits for its own read, applies it and goes back for another, so a slot
  // frees where it was taken rather than through a count kept beside the requests.
  const pump = useCallback(() => {
    const reads = readsRef.current;
    while (reads.workers < CONCURRENCY && reads.queue.length > 0) {
      reads.workers += 1;
      void (async () => {
        try {
          for (
            let modId = reads.queue.shift();
            modId;
            modId = reads.queue.shift()
          ) {
            const result = await getRepositoryModSourceData({ modId });
            if (result.status !== 'reply' || readsRef.current !== reads) {
              // The read went away with the mount that made it - abandoned by
              // the IPC hook, or answered anyway by a mock host, which cancels
              // nothing - and this worker goes with it: the mods it was for are
              // left loading with nothing open, which is what the fill effect
              // asks for again.
              return;
            }
            reads.open.delete(modId);
            const source = result.data.data.source;
            // An unreachable repository reports itself as a null source, which is
            // how the details screen decides its own "loading failed" too.
            patchSource(
              modId,
              source
                ? {
                    status: 'ready',
                    source,
                    version: result.data.data.metadata?.version,
                  }
                : { status: 'failed', source: undefined, version: undefined }
            );
          }
        } finally {
          reads.workers -= 1;
        }
      })();
    }
  }, [getRepositoryModSourceData, patchSource]);

  // A mod needs fetching when nothing is known about it, and again when what is
  // known is that it is loading with nothing open for it - which is the state a
  // read abandoned with its mount leaves behind.
  useEffect(() => {
    const reads = readsRef.current;
    const fresh = modIds.filter((modId) => {
      const current = sourcesRef.current[modId];
      return (!current || current.status === 'loading') && !reads.open.has(modId);
    });
    if (fresh.length === 0) {
      return;
    }

    const next = { ...sourcesRef.current };
    for (const modId of fresh) {
      next[modId] = { ...next[modId], status: 'loading' };
      reads.open.add(modId);
    }
    writeSources(next);
    reads.queue.push(...fresh);
    pump();
  }, [modIds, pump, writeSources]);

  // Let go of the mount's reads with the mount, which leaves the mods they were
  // for loading with nothing open and the effect above asking again. A mount
  // taken down and set up again - which is what StrictMode does to every screen,
  // and what the app mounts under - would otherwise leave each row waiting on a
  // reply that was cancelled, the record of having asked being the thing that
  // keeps it from asking.
  //
  // An effect of its own rather than the cleanup of the one above, which re-runs
  // whenever the list changes, where the reads in flight belong to a mount that
  // is still there and are still coming.
  useEffect(
    () => () => {
      readsRef.current = newSourceReads();
    },
    []
  );

  // Fetch one mod's repository source again, for a row whose first attempt failed.
  const retry = useCallback(
    (modId: string) => {
      if (sourcesRef.current[modId]?.status !== 'failed') {
        return;
      }
      patchSource(modId, { status: 'loading' });
      readsRef.current.open.add(modId);
      readsRef.current.queue.push(modId);
      pump();
    },
    [patchSource, pump]
  );

  const { getModSourceData } = useGetModSourceData();

  // Fetch one mod's installed source, for the diff. Does nothing for a mod already
  // being fetched or already fetched, so opening the tab twice is one round trip;
  // `force` is the retry a failed fetch offers.
  const loadInstalledSource = useCallback(
    async (modId: string, force = false) => {
      const current = sourcesRef.current[modId];
      if (!current) {
        return;
      }
      if (
        !force &&
        (current.installedStatus === 'loading' ||
          current.installedStatus === 'ready')
      ) {
        return;
      }
      patchSource(modId, { installedStatus: 'loading' });

      const result = await getModSourceData({ modId });
      if (result.status !== 'reply') {
        return;
      }
      const source = result.data.data.source;
      patchSource(modId, {
        installedStatus: source ? 'ready' : 'failed',
        installedSource: source ?? undefined,
      });
    },
    [getModSourceData, patchSource]
  );

  return { sources, retry, loadInstalledSource };
}
