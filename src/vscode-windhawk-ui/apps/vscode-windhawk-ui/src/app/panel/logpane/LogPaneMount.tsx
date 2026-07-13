// Keeps Monaco out of app startup: this tiny component (no Monaco import) owns the
// shell's reveal signal and the pane's visibility, and lazy-loads the heavy Monaco
// LogPane only once the log is first revealed. It then stays mounted so the model
// survives closing and reopening; visibility is toggled via the `visible` prop.
//
// Rendered only in the Tauri build (the log pane is native-shell specific); see
// Panel.tsx.

import { lazy, Suspense, useEffect, useState } from 'react';
import { listenLogShow, type UnlistenFn } from '../../tauriApi';

const LogPane = lazy(() => import('./LogPane'));

function LogPaneMount() {
  // `activated` latches true on the first reveal so Monaco is fetched once and the
  // pane stays mounted; `visible` shows/hides it thereafter.
  const [activated, setActivated] = useState(false);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    listenLogShow(() => {
      setActivated(true);
      setVisible(true);
    })?.then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  if (!activated) {
    return null;
  }

  return (
    <Suspense fallback={null}>
      <LogPane visible={visible} onClose={() => setVisible(false)} />
    </Suspense>
  );
}

export default LogPaneMount;
