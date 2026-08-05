import { useCallback, useState } from 'react';

/**
 * The close state of a modal its caller mounts only while it is up.
 *
 * antd animates the dialog out when `open` goes from true to false, which a caller
 * that drops the element on the close never gets to show: React takes the DOM away
 * in the same commit, leaving nothing to animate. So the dismissal only lowers
 * `open` here, and the caller's `onClose` is reported from the modal's `afterClose`
 * - the same unmount, one animation later.
 *
 * Wire all three: `open` and `afterClose` to the modal, and `close` to every control
 * that dismisses it, in place of `onClose`.
 */
export function useModalClose(onClose: () => void) {
  const [open, setOpen] = useState(true);
  const close = useCallback(() => setOpen(false), []);
  return { open, close, afterClose: onClose };
}

export default useModalClose;
