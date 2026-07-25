// Selects the IPC transport at build time. The Tauri build bridges to the
// native windhawk-ui shell; every other build uses the VSCode webview API (null
// in website mode, where it must not be used). Both are imported, but
// WEBPACK_IS_TAURI is a DefinePlugin constant, so webpack drops the dead branch
// (and tree-shakes the unused transport) per build.
import tauriApi from './tauriApi';
import vsCodeApi from './vsCodeApi';

declare const WEBPACK_IS_TAURI: boolean;

const backendApi = WEBPACK_IS_TAURI ? tauriApi : vsCodeApi;

export default backendApi;
