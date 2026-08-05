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

  // The mods waiting for a slot, and how many fetches are open.
  const queueRef = useRef<string[]>([]);
  const inFlightRef = useRef(0);
  // Held in a ref so `pump` can stay identity-stable: the reply handler calls it,
  // and the handler is what decides when the next slot frees up.
  const fetchRef = useRef<(data: { modId: string }) => void>();

  const pump = useCallback(() => {
    while (inFlightRef.current < CONCURRENCY && queueRef.current.length > 0) {
      const modId = queueRef.current.shift() as string;
      inFlightRef.current += 1;
      fetchRef.current?.({ modId });
    }
  }, []);

  const { getRepositoryModSourceData } = useGetRepositoryModSourceData(
    useCallback(
      (data) => {
        // One decrement per fetch, which holds on both sides: a reply reaches
        // this handler only for a request `pump` counted in, and no reply is
        // dropped on the way, the hook discarding one only when a newer request
        // for the same mod has been answered. At most one fetch per mod is ever
        // open - a mod is queued once, and `retry` is offered only for a mod
        // whose reply has already landed - so there is no newer request to be
        // answered first.
        inFlightRef.current -= 1;
        const source = data.data.source;
        // An unreachable repository reports itself as a null source, which is how
        // the details screen decides its own "loading failed" too.
        patchSource(
          data.modId,
          source
            ? {
                status: 'ready',
                source,
                version: data.data.metadata?.version,
              }
            : { status: 'failed', source: undefined, version: undefined }
        );
        pump();
      },
      [patchSource, pump]
    )
  );

  // Ahead of the effect that fills the queue, so the first pump has something to
  // call.
  useEffect(() => {
    fetchRef.current = getRepositoryModSourceData;
  }, [getRepositoryModSourceData]);

  useEffect(() => {
    const fresh = modIds.filter((modId) => !(modId in sourcesRef.current));
    if (fresh.length === 0) {
      return;
    }

    const next = { ...sourcesRef.current };
    for (const modId of fresh) {
      next[modId] = { status: 'loading' };
    }
    writeSources(next);
    queueRef.current.push(...fresh);
    pump();
  }, [modIds, pump, writeSources]);

  // Fetch one mod's repository source again, for a row whose first attempt failed.
  const retry = useCallback(
    (modId: string) => {
      if (sourcesRef.current[modId]?.status !== 'failed') {
        return;
      }
      patchSource(modId, { status: 'loading' });
      queueRef.current.push(modId);
      pump();
    },
    [patchSource, pump]
  );

  const { getModSourceData } = useGetModSourceData(
    useCallback(
      (data) => {
        const source = data.data.source;
        patchSource(data.modId, {
          installedStatus: source ? 'ready' : 'failed',
          installedSource: source ?? undefined,
        });
      },
      [patchSource]
    )
  );

  // Fetch one mod's installed source, for the diff. Does nothing for a mod already
  // being fetched or already fetched, so opening the tab twice is one round trip;
  // `force` is the retry a failed fetch offers.
  const loadInstalledSource = useCallback(
    (modId: string, force = false) => {
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
      getModSourceData({ modId });
    },
    [getModSourceData, patchSource]
  );

  return { sources, retry, loadInstalledSource };
}
