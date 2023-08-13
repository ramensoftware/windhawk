import vsCodeApi from '../vsCodeApi';

export const useMockData = !vsCodeApi;

export const mockSettings = !useMockData
  ? null
  : {
      language: 'en',
      disableUpdateCheck: false,
      disableRunUIScheduledTask: false,
      devModeOptOut: false,
      devModeUsedAtLeastOnce: false,
      hideTrayIcon: false,
      dontAutoShowToolkit: false,
      modTasksDialogDelay: 2000,
      injectIntoCriticalProcesses: false,
      safeMode: false,
      loggingVerbosity: 0,
      engine: {
        loggingVerbosity: 0,
        include: ['a.exe', 'b.exe'],
        exclude: ['c.exe', 'd.exe'],
        threadAttachExempt: ['e.exe', 'f.exe'],
      },
    };

const mockModMetadata = {
  id: 'custom-message-box',
  name: 'Custom Message Box',
  description: 'Customizes the message box',
  version: '0.1',
  author: 'Michael Jackson',
  github: 'https://github.com/jackson',
  twitter: 'https://twitter.com/jackson',
  homepage: 'http://custom-message-box.com/',
  include: ['*'],
  exclude: ['explorer.exe'],
};

const mockModMetadataOnline = {
  ...mockModMetadata,
  id: undefined,
  version: '0.2',
};

const mockModConfig = {
  libraryFileName: 'custom-message-box-123456.dll',
  disabled: false,
  loggingEnabled: false,
  debugLoggingEnabled: false,
  include: ['*'],
  exclude: ['explorer.exe'],
  includeCustom: [],
  excludeCustom: [],
  architecture: ['x86-64'],
  version: '1.0',
};

const mockModDetails = {
  metadata: {},
  config: mockModConfig,
  updateAvailable: false,
  userRating: 0,
};

export const mockModsBrowserLocalInitialMods = !useMockData
  ? null
  : {
      'custom-message-box': {
        metadata: mockModMetadata,
        config: mockModConfig,
        updateAvailable: true,
        userRating: 4,
      },
      'local@asdf2': mockModDetails,
      asdf3: mockModDetails,
      asdf4: mockModDetails,
      asdf5: mockModDetails,
      asdf6: mockModDetails,
      asdf7: mockModDetails,
    };

export const mockModsBrowserLocalFeaturedMods = !useMockData
  ? null
  : {
      online1: {
        metadata: mockModMetadataOnline,
        details: {
          users: 111222333,
          rating: 5,
          defaultSorting: 2,
          published: 1618321977408,
          updated: 1718321977408,
        },
      },
    };

export const mockModsBrowserOnlineRepositoryMods = !useMockData
  ? null
  : {
      online1: {
        repository: {
          metadata: mockModMetadataOnline,
          details: {
            users: 111222333,
            rating: 5,
            defaultSorting: 2,
            published: 1618321977408,
            updated: 1718321977408,
          },
        },
        installed: {
          metadata: mockModMetadata,
          config: mockModConfig,
        },
      },
      ...Object.fromEntries(
        Array(100)
          .fill(undefined)
          .map((e, i) => [
            `online${(i + 1).toString().padStart(3, '0')}`,
            {
              repository: {
                metadata: {
                  name: `My Mod ${(i + 1).toString().padStart(3, '0')}`,
                  description: 'A good mod',
                  version: '1.2',
                  author: 'John Smith',
                  github: 'https://github.com/john',
                  twitter: 'https://twitter.com/john',
                  homepage: 'https://example.com/',
                },
                details: {
                  users: 20,
                  rating: 7,
                  defaultSorting: 1,
                  published: 1618321977408,
                  updated: 1718321977408,
                },
              },
            },
          ])
      ),
    };
