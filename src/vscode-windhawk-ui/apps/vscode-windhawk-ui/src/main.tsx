import { StrictMode } from 'react';
import * as ReactDOM from 'react-dom/client';
import App from './app/app';
import './main.css';

const params = document.querySelector('body')?.getAttribute('data-params');
const previewModId = params && JSON.parse(params).previewModId;
if (previewModId) {
  const url = new URL(window.location.href);
  url.hash = '#/mod-preview/' + previewModId;
  window.history.replaceState(null, '', url);
}

const root = ReactDOM.createRoot(
  document.getElementById('root') as HTMLElement
);
root.render(
  <StrictMode>
    <App />
  </StrictMode>
);
