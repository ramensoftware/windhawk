// Use webpack constant for conditional compilation
declare const WEBPACK_IS_WEBSITE: boolean;

// https://github.com/microsoft/vscode/issues/96221#issuecomment-735408921
declare function acquireVsCodeApi<T = unknown>(): {
  getState: () => T;
  setState: (data: T) => void;
  postMessage: (msg: unknown) => void;
};

const websiteApi = {
  getState: () => {
    throw new Error('VSCode API must not be used in website mode');
  },
  setState: () => {
    throw new Error('VSCode API must not be used in website mode');
  },
  postMessage: () => {
    throw new Error('VSCode API must not be used in website mode');
  },
};

const vsCodeApi = WEBPACK_IS_WEBSITE
  ? websiteApi
  : typeof acquireVsCodeApi !== 'undefined'
    ? acquireVsCodeApi()
    : null;

export default vsCodeApi;
