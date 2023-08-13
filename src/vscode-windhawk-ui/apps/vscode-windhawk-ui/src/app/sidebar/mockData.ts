import vsCodeApi from '../vsCodeApi';

export const useMockData = !vsCodeApi;

export const mockSidebarModDetails = !useMockData
  ? null
  : {
      modId: 'new-mod-test',
      modWasModified: false,
      compiled: true,
      disabled: false,
      loggingEnabled: false,
      debugLoggingEnabled: false,
    };
