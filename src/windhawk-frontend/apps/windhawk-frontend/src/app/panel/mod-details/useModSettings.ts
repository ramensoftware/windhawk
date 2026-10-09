import { isWireError } from '@app/feedback';
import { useGetModSettings, useSetModSettings } from '@app/webviewIPC';
import { useCallback, useEffect, useLayoutEffect, useRef } from 'react';
import { useReadWriteGuard } from '../shared/useReadWriteGuard';
import { useResync } from '../shared/useResync';
import { type ModSettings } from './tabs/settings/core/yamlConverter';

type Options = {
  // False for a view with nothing to read, which then reads nothing and
  // subscribes to nothing.
  enabled?: boolean;
  // Handed each read that is applied. `first` marks the first read of the mod,
  // the one a view is drawn from; a later one is the values moved under a view
  // already drawn.
  onRead: (settings: ModSettings, first: boolean) => void;
};

/**
 * A mod's settings values as the machine holds them, and the way to write them:
 * one owner for every view that draws them.
 *
 * The values are read on mount, for the mod switched to, when the window comes
 * back (the CLI, another window or the editor can write them meanwhile) and on
 * `reload`, all through the one queue, so no two reads resolve out of order. A
 * read is applied only for the mod the owner is on, and, after the mod's first,
 * only where no write of this owner's overtook it (see useReadWriteGuard).
 *
 * App-only - it uses the settings IPC hooks, which are unavailable in website
 * builds.
 */
export function useModSettings(
  modId: string,
  { enabled = true, onRead }: Options
) {
  const { getModSettings } = useGetModSettings();
  const { setModSettings, setModSettingsPending } = useSetModSettings();
  const { mark, write, overtaken } = useReadWriteGuard();

  // What a reply is judged against when it lands, rather than what its request
  // closed over: the mod the owner is on, and the handler as it stands.
  const modIdRef = useRef(modId);
  const onReadRef = useRef(onRead);
  useLayoutEffect(() => {
    modIdRef.current = modId;
    onReadRef.current = onRead;
  });

  // The mod the last read applied was for. A mod's first read is applied
  // whatever this owner has written meanwhile: nothing is written to a mod
  // before it is read (see save), so a write out then is another mod's.
  const readForRef = useRef<string | null>(null);

  const read = useCallback(async () => {
    const at = mark();
    const result = await getModSettings({ modId });
    if (result.status !== 'reply' || result.data.modId !== modIdRef.current) {
      return;
    }
    const first = readForRef.current !== result.data.modId;
    // A read the host could not make carries an empty map, which says nothing
    // of the values a view already holds.
    if (!first && (isWireError(result.data.error) || overtaken(at))) {
      return;
    }
    readForRef.current = result.data.modId;
    onReadRef.current(result.data.settings, first);
  }, [getModSettings, modId, mark, overtaken]);

  const reload = useResync(read, { onMount: true, enabled });

  // A mod switched to is read as one opened.
  const shownModIdRef = useRef(modId);
  useEffect(() => {
    if (shownModIdRef.current !== modId) {
      shownModIdRef.current = modId;
      reload();
    }
  }, [modId, reload]);

  // Resolves with whether the host took the values. An abandoned request
  // reports nothing, and a reply for a mod the owner has left is not one for
  // the view now drawn to take as saved. A view still drawn from the mod before
  // has none of this one's values to save, so nothing goes out until the mod
  // has been read.
  const save = useCallback(
    async (settings: ModSettings) => {
      if (readForRef.current !== modId) {
        return false;
      }
      const result = await write(() => setModSettings({ modId, settings }));
      return (
        result.status === 'reply' &&
        result.data.modId === modIdRef.current &&
        result.data.succeeded
      );
    },
    [modId, write, setModSettings]
  );

  return { reload, save, saving: setModSettingsPending };
}
