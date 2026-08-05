import { useCancelCompileMod, useCancelInstallMod } from '@app/webviewIPC';
import { useCallback, useEffect, useRef } from 'react';

// The context both browsers attach to an install or a compile. `modId` is what the
// cancel needs: the operation the modal covers has to be named on the wire, because
// the host runs one install per mod and several can be in flight. `updating`
// distinguishes an update from a first install for the modal's label.
export type ModOperationContext = {
  modId: string;
  updating?: boolean;
};

type Args = {
  installModPending?: boolean;
  installModContext?: ModOperationContext;
  compileModPending?: boolean;
  compileModContext?: ModOperationContext;
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
 * It always resolves, including where no ack ever comes: an operation that ends
 * while the cancel is still on the wire, and a screen that goes away with one in
 * flight, both answer it themselves.
 *
 * A cancel that is taken up is cooperative: the operation keeps running until it
 * reaches a point where it can stop, and only its own reply ends the modal.
 */
export function useCancelModOperation({
  installModPending,
  installModContext,
  compileModPending,
  compileModContext,
}: Args) {
  // The ack the cancel in flight is waiting on. One at a time: the caller stops
  // offering the cancel until the one it sent has been answered.
  const ackRef = useRef<(succeeded: boolean) => void>();
  const releaseAck = useCallback((succeeded: boolean) => {
    ackRef.current?.(succeeded);
    ackRef.current = undefined;
  }, []);

  const settleAck = useCallback(
    (data: { succeeded: boolean }) => releaseAck(data.succeeded),
    [releaseAck]
  );

  const { cancelInstallMod } = useCancelInstallMod(settleAck);
  const { cancelCompileMod } = useCancelCompileMod(settleAck);

  // The host's reply is not the only end a cancel can meet. The operation it names
  // can reach its own first, and the screen can go away with the cancel in flight;
  // the reply is then moot or has nowhere to land, and the waiter would keep
  // waiting. Answer it here, as taken up - not a claim about what the host did, but
  // what both cases leave the caller with: no operation to go on offering a cancel
  // for.
  const operationPending = !!(installModPending || compileModPending);
  useEffect(() => {
    if (!operationPending) {
      releaseAck(true);
    }
  }, [operationPending, releaseAck]);
  useEffect(() => () => releaseAck(true), [releaseAck]);

  return useCallback(() => {
    // At most one of the two is pending, the modal being what keeps a second
    // operation from being started; the install is asked about first as a stable
    // tie-break. Its context names the newest install of the set `pending` covers,
    // which is the one the modal went up for.
    const modId = installModPending
      ? installModContext?.modId
      : compileModPending
        ? compileModContext?.modId
        : undefined;
    if (!modId) {
      return Promise.resolve(false);
    }

    const ack = new Promise<boolean>((resolve) => {
      ackRef.current = resolve;
    });

    if (installModPending) {
      cancelInstallMod({ modId });
    } else {
      cancelCompileMod({ modId });
    }

    return ack;
  }, [
    installModPending,
    installModContext,
    compileModPending,
    compileModContext,
    cancelInstallMod,
    cancelCompileMod,
  ]);
}
