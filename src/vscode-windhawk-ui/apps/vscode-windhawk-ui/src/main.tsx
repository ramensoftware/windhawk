import { StrictMode } from 'react';
import * as ReactDOM from 'react-dom/client';
import App from './app/app';
import './main.css';
import { initTauriBridge } from './app/tauriApi';
/// #if HAS_MOCKS
import { MockProvider } from './app/mocking';
/// #endif

declare const WEBPACK_IS_TAURI: boolean;

// Tauri build only: register the inbound wh-ipc bridge before the app mounts, so
// the first replies/events (e.g. getInitialAppSettings) are delivered. The
// constant is a DefinePlugin literal, so other builds drop this branch and
// tree-shake the import.
if (WEBPACK_IS_TAURI) {
  initTauriBridge();
}

const root = ReactDOM.createRoot(
  document.getElementById('root') as HTMLElement
);
root.render(
  <StrictMode>
    {
      /// #if HAS_MOCKS
      <MockProvider>
        <App />
      </MockProvider>
      /// #endif
    }
    {
      /// #if !HAS_MOCKS
      <App />
      /// #endif
    }
  </StrictMode>
);
