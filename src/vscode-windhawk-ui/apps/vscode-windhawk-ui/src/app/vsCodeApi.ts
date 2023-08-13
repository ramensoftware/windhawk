// https://github.com/microsoft/vscode/issues/96221#issuecomment-735408921
declare function acquireVsCodeApi<T = unknown>(): {
  getState: () => T;
  setState: (data: T) => void;
  postMessage: (msg: unknown) => void;
};

const vsCodeApi =
  typeof acquireVsCodeApi !== 'undefined' ? acquireVsCodeApi() : null;

export default vsCodeApi;
