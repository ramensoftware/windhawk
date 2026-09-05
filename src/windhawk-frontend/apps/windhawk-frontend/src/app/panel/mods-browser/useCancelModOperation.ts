import { useCancelCompileMod, useCancelInstallMod } from '@app/webviewIPC';
import { useCallback, useEffect, useRef } from 'react';
import { type ModOperation } from './modOperation';

type Args = {
  installModPending?: boolean;
  compileModPending?: boolean;
  // What the screen is running, which is what the cancel names on the wire.
  operation?: ModOperation;
};

/**
 * Cancel whichever install or compile the mods-browser modal is currently
 * covering. A screen runs one of them at a time - the modal blocks the rest of the
 * UI while it is up - which is what makes "the operation" unambiguous here; the
 * command still names the mod, because the host has no such notion.
 *
 * Resolves to whether the cancel was taken up. A false is the caller's cue to keep
 * offering it: either there was no operation to name, or the host answered that it
 * found none in flight for that mod. The latter is not only the harmless race with
 * an operation that just settled - a cancel can also arrive before the host has
 * registered the operation it is about, which it does only after reading and
 * parsing the mod source, and that one leaves an operation running.
 *
 * It always resolves, including where the ack is beside the point: an operation
 * that ends while the cancel is still on the wire is the race below, and a screen
 * that goes away with one in flight abandons it.
 *
 * A cancel that is taken up is cooperative: the operation keeps running until it
 * reaches a point where it can stop, and only its own reply ends the modal.
 */
export function useCancelModOperation({
  installModPending,
  compileModPending,
  operation,
}: Args) {
  const { cancelInstallMod } = useCancelInstallMod();
  const { cancelCompileMod } = useCancelCompileMod();

  // The end the host has nothing to answer for: the operation the cancel names
  // can reach its own first, and the ack is then moot while the caller is still
  // waiting on it. Held as a resolver so the ask below can race its own reply
  // against it, and answered as taken up - not a claim about what the host did,
  // but what it leaves the caller with: no operation to go on offering a cancel
  // for.
  const operationEndedRef = useRef<(taken: boolean) => void>();
  const operationPending = !!(installModPending || compileModPending);
  useEffect(() => {
    if (!operationPending) {
      operationEndedRef.current?.(true);
      operationEndedRef.current = undefined;
    }
  }, [operationPending]);

  return useCallback(async () => {
    // At most one of the two is pending, the modal being what keeps a second
    // operation from being started; the install is asked about first as a stable
    // tie-break.
    const modId = operationPending ? operation?.modId : undefined;
    if (!modId) {
      return false;
    }

    const operationEnded = new Promise<boolean>((resolve) => {
      operationEndedRef.current = resolve;
    });

    const acked = (
      installModPending
        ? cancelInstallMod({ modId })
        : cancelCompileMod({ modId })
    ).then(
      // An abandoned request is the screen going away with the cancel still on
      // the wire, which leaves nothing to go on offering it for either.
      (result) => result.status !== 'reply' || result.data.succeeded
    );

    return Promise.race([acked, operationEnded]);
  }, [
    operationPending,
    operation,
    installModPending,
    cancelInstallMod,
    cancelCompileMod,
  ]);
}
