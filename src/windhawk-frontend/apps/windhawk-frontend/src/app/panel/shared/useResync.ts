import { useReloadInstalledMods } from '@app/webviewIPC';
import { useCallback, useEffect, useLayoutEffect, useRef } from 'react';

type Queue = {
  live: boolean;
  running: boolean;
  queued: boolean;
};

/**
 * Runs `handler` when the window comes back, or when the host says the machine
 * may have changed under it.
 *
 * Machine state subscribes: a cheap local read from the host, of something the
 * CLI, another window or the editor can change while this window is away.
 * Network data is not read again on return, and session constants are read
 * once.
 *
 * The host's event covers what focus does not: in VSCode the panel can come
 * back without a focus event of its own, and the editor writes to the machine
 * with the panel in the background. The Tauri host sends none, so there focus
 * is the whole of it.
 *
 * One run at a time, so two reads of the same thing cannot resolve out of
 * order. A trigger that lands during a run is not dropped, since the read
 * already out may predate the write the trigger follows: it queues one more run
 * for after this one, and later triggers fold into that. A window coming back
 * in VSCode can bring both a focus and the host's event, which costs two reads.
 * `onMount` runs the handler on mount as well, as the first run of the same
 * queue, for a caller whose first read is the one it reads again.
 *
 * Returns a function that asks for a run the way a trigger does, for a caller
 * with a reason of its own to read again: its read then cannot resolve out of
 * order with the queue's. It runs nothing before mount, after unmount, or while
 * `enabled` is false, which also leaves the triggers and `onMount` with nothing
 * to run.
 *
 * App-only: the host's event throws in the website build.
 */
export function useResync(
  handler: () => Promise<unknown> | void,
  {
    onMount = false,
    enabled = true,
  }: { onMount?: boolean; enabled?: boolean } = {}
) {
  // The latest handler is the one run, so a caller need not hold it still.
  const handlerRef = useRef(handler);
  useLayoutEffect(() => {
    handlerRef.current = handler;
  }, [handler]);

  // The queue of the current mount, so a run left over from an unmount starts
  // nothing after it.
  const queueRef = useRef<Queue | null>(null);

  const trigger = useCallback(() => {
    const queue = queueRef.current;
    if (!queue) {
      return;
    }
    if (queue.running) {
      queue.queued = true;
      return;
    }
    void drain(queue, () => handlerRef.current());
  }, []);

  useEffect(() => {
    if (!enabled) {
      return;
    }
    const queue: Queue = { live: true, running: false, queued: false };
    queueRef.current = queue;
    window.addEventListener('focus', trigger);
    if (onMount) {
      void drain(queue, () => handlerRef.current());
    }
    return () => {
      window.removeEventListener('focus', trigger);
      queue.live = false;
      queueRef.current = null;
    };
  }, [trigger, onMount, enabled]);

  useReloadInstalledMods(trigger);

  return trigger;
}

async function drain(queue: Queue, run: () => Promise<unknown> | void) {
  queue.running = true;
  do {
    queue.queued = false;
    try {
      await run();
    } catch (error) {
      console.error('Resync failed:', error);
    }
  } while (queue.queued && queue.live);
  queue.running = false;
}
