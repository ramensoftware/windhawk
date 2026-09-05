import { useCallback, useState } from 'react';

/**
 * The install or compile a mods browser is running. `modId` is what a cancel
 * needs: the host runs one install per mod and several can be in flight, so the
 * command alone would not say which to stop. `updating` distinguishes an update
 * from a first install, for the progress modal's label.
 */
export type ModOperation = {
  modId: string;
  updating?: boolean;
};

/**
 * What the progress modal is covering, held by the screen that started it. A
 * request answers the caller that made it, and this is the other question: what
 * this screen has running, which the modal names and the cancel button targets.
 */
export function useModOperation() {
  const [operation, setOperation] = useState<ModOperation>();

  // Set as the operation is posted and dropped as it settles, which is a beat
  // after `pending` goes false - so the modal reads the operation's own label
  // for as long as it is on screen animating out.
  const track = useCallback((next: ModOperation, posted: Promise<unknown>) => {
    setOperation(next);
    posted.finally(() => setOperation(undefined));
  }, []);

  return { operation, track };
}
