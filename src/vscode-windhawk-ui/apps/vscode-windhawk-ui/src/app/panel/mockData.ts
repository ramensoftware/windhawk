import vsCodeApi from '../vsCodeApi';

export const useMockData = !vsCodeApi;

export const mockAppUISettings = !useMockData
  ? null
  : {
    language: 'en',
    devModeOptOut: false,
    devModeUsedAtLeastOnce: false,
    loggingEnabled: false,
    updateIsAvailable: false,
    safeMode: false,
  };

export const mockSettings = !useMockData
  ? null
  : {
    language: 'en',
    disableUpdateCheck: false,
    disableRunUIScheduledTask: false,
    devModeOptOut: false,
    devModeUsedAtLeastOnce: false,
    hideTrayIcon: false,
    alwaysCompileModsLocally: false,
    dontAutoShowToolkit: false,
    modTasksDialogDelay: 2000,
    safeMode: false,
    loggingVerbosity: 0,
    engine: {
      loggingVerbosity: 0,
      include: ['a.exe', 'b.exe'],
      exclude: ['c.exe', 'd.exe'],
      injectIntoCriticalProcesses: false,
      injectIntoIncompatiblePrograms: false,
      injectIntoGames: false,
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
  license: 'MIT',
  donateUrl: 'https://example.com/donate',
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
  includeExcludeCustomOnly: false,
  patternsMatchCriticalSystemProcesses: false,
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
        ratingBreakdown: [1, 2, 16, 3, 5],
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
          ratingBreakdown: [1, 2, 16, 3, 5],
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
                ratingBreakdown: [1, 2, 4, 8, 16],
                defaultSorting: 1,
                published: 1618321977408,
                updated: 1718321977408,
              },
            },
          },
        ])
    ),
  };

export const mockInstalledModSourceData = !useMockData
  ? null
  : {
    source: '// Mock local source...\n',
    metadata: mockModMetadata,
    readme: `# Mock readme...

| Month    | Savings |
| -------- | ------- |
| January  | $250    |
| February | $80     |
| March    | $420    |

More text...`,
    initialSettings: [
      {
        key: 'mock-setting',
        value: 'mock-setting-value',
        name: 'Mock Setting Name',
        description: 'Mock setting description',
      },
      {
        key: 'mock-setting-dropdown',
        value: 'a',
        name: 'Mock Setting Dropdown Name',
        description: 'Mock setting dropdown description',
        options: [
          { a: 'a option' },
          { b: 'b option' },
          { c: 'c option' },
          { d: 'd option' },
          { e: 'e option' },
          { f: 'f option' },
          { g: 'g option' },
          { h: 'h option' },
          { i: 'i option' },
        ],
      },
      {
        key: 'mock-setting-array',
        value: ['a', 'b', 'c'],
        name: 'Mock Setting Array Name',
        description: 'Mock setting array description',
      },
      {
        key: 'mock-setting-nested-array',
        value: [
          [

            {
              key: 'mock-setting-nested',
              value: ['a', 'b', 'c'],
              name: 'Mock Setting Nested Name',
              description: 'Mock setting nested description',
            }
          ]
        ],
        name: 'Mock Setting Nested Array Name',
        description: 'Mock setting nested array description',
      },
    ],
  };

export const mockModSettings = !useMockData
  ? null
  : {
    'mock-setting': 'mock-setting-value',
    'mock-setting-dropdown': 'mock-setting-value',
    'mock-setting-array[0]': 'a',
    'mock-setting-array[1]': 'b',
    'mock-setting-array[2]': 'c',
  };

export const mockModVersions = !useMockData
  ? null
  : [
    {
      version: '0.3-alpha',
      timestamp: 1758321977, // Sep 20, 2025
      isPreRelease: true,
    },
    {
      version: '0.2',
      timestamp: 1718321977, // Jun 14, 2024
      isPreRelease: false,
    },
    {
      version: '0.1',
      timestamp: 1690444800, // Jul 27, 2023
      isPreRelease: false,
    },
    {
      version: '0.1-beta',
      timestamp: 1684454400, // May 19, 2023
      isPreRelease: true,
    },
  ];

export const mockModVersionSource = !useMockData
  ? null
  : (version: string) => ({
    source: `// Mock source for version ${version}...\n`,
    metadata: {
      ...mockModMetadata,
      version,
    },
    readme: `# Mock readme for version ${version}...\n`,
    initialSettings: [],
  });
