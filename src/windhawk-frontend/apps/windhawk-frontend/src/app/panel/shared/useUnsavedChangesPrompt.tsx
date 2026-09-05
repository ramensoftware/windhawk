import { Modal } from 'antd';
import { useCallback, useRef } from 'react';

interface Strings {
  title: string;
  message: string;
  // What the button that gives the changes up says.
  leave: string;
  // What the button that goes back to them says.
  stay: string;
}

/**
 * Asks whether to give up unsaved changes, as a promise: true to leave them
 * behind, false to keep them.
 *
 * An ask made while one is already on screen is answered false rather than
 * stacking a second dialog over the first - two ways to leave can reach this at
 * once (a route change and a tab switch), and the one the user is looking at is
 * the one that should decide.
 */
export function useUnsavedChangesPrompt({ title, message, leave, stay }: Strings) {
  const isOpen = useRef(false);

  return useCallback((): Promise<boolean> => {
    if (isOpen.current) {
      return Promise.resolve(false);
    }

    isOpen.current = true;

    return new Promise((resolve) => {
      // Whichever of the three lands first is the answer; the others are then
      // no-ops. `afterClose` is there for the dismissals that report neither -
      // a `Modal.destroyAll()` closes the dialog without an `onCancel` - and
      // those are the ones that matter most: an ask left unsettled holds the
      // caller awaiting it forever, and `isOpen` with it, so every later ask
      // answers false and the screen can no longer be left at all.
      const settle = (answer: boolean) => {
        isOpen.current = false;
        resolve(answer);
      };

      Modal.confirm({
        title,
        content: message,
        okText: leave,
        cancelText: stay,
        onOk: () => settle(true),
        onCancel: () => settle(false),
        afterClose: () => settle(false),
        closable: true,
        maskClosable: true,
      });
    });
  }, [title, message, leave, stay]);
}
