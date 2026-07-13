import {
  type AppSettings,
  type AppUISettings,
  type GetFeaturedModsReplyData,
  type GetInstalledModsReplyData,
  type GetModVersionsReplyData,
  type GetRepositoryModsReplyData,
  type InitialSettings,
  type ModConfig,
  type ModMetadata,
  type SetEditedModDetailsData,
} from '@app/webviewIPCMessages';

/**
 * Centralized registry of all mock data used for development mode.
 * This replaces the scattered mockData.ts files throughout the application.
 */

// ============================================================================
// Type Definitions
// ============================================================================

// Re-export types from IPC messages for convenience
export type ModDetailsType = GetInstalledModsReplyData['installedMods'][string];
export type FeaturedModDetailsType = NonNullable<GetFeaturedModsReplyData['featuredMods']>[string];
export type RepositoryModType = NonNullable<GetRepositoryModsReplyData['mods']>[string];
export type ModVersion = GetModVersionsReplyData['versions'][number];
export type SidebarModDetails = SetEditedModDetailsData;

// Custom type for mod source data structure (used in multiple places)
export interface InstalledModSourceData {
  source: string;
  metadata: ModMetadata;
  readme: string;
  initialSettings: InitialSettings;
}

/**
 * Complete registry of all mock data used throughout the application
 */
export interface MockDataRegistry {
  // App-level settings
  appUISettings: AppUISettings;
  appSettings: AppSettings;

  // Mods browser - Local
  installedMods: Record<string, ModDetailsType>;
  featuredMods: Record<string, FeaturedModDetailsType>;

  // Mods browser - Online
  repositoryMods: Record<string, RepositoryModType>;

  // Mod details
  installedModSourceData: InstalledModSourceData;
  modSettings: Record<string, unknown>;
  modVersions: ModVersion[];
  modVersionSource: (version: string) => InstalledModSourceData;
  modConfig: Record<string, ModConfig>;

  // Sidebar (editor mode)
  sidebarModDetails: SidebarModDetails;
}

// ============================================================================
// Mock Data Definitions
// ============================================================================

const mockModMetadata: ModMetadata = {
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

const mockModMetadataOnline: ModMetadata = {
  ...mockModMetadata,
  version: '0.2',
};

const mockModConfig: ModConfig = {
  disabled: false,
  loggingEnabled: false,
  debugLoggingEnabled: false,
  include: ['*'],
  exclude: ['explorer.exe'],
  includeCustom: [],
  excludeCustom: [],
  includeExcludeCustomOnly: false,
  patternsMatchCriticalSystemProcesses: true,
  architecture: ['x86-64'],
  version: '1.0',
};

const mockModDetails: ModDetailsType = {
  metadata: {},
  config: mockModConfig,
  updateAvailable: false,
  userRating: 0,
};

/**
 * Default mock data registry with realistic test data for development mode
 */
export const defaultMockData: MockDataRegistry = {
  // ============================================================================
  // App-level settings
  // ============================================================================

  appUISettings: {
    language: 'en',
    devModeOptOut: false,
    loggingEnabled: false,
    updateIsAvailable: false,
    updateIsAvailableBleedingEdge: false,
    safeMode: false,
  },

  appSettings: {
    language: 'en',
    disableUpdateCheck: false,
    disableRunUIScheduledTask: false,
    devModeOptOut: false,
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
  },

  // ============================================================================
  // Mods browser - Local
  // ============================================================================

  installedMods: {
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
  },

  featuredMods: {
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
  },

  // ============================================================================
  // Mods browser - Online
  // ============================================================================

  repositoryMods: {
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
        userRating: 4,
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
  },

  // ============================================================================
  // Mod details
  // ============================================================================

  installedModSourceData: {
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
          { a: 'a option' } as Record<string, string>,
          { b: 'b option' } as Record<string, string>,
          { c: 'c option' } as Record<string, string>,
          { d: 'd option' } as Record<string, string>,
          { e: 'e option' } as Record<string, string>,
          { f: 'f option' } as Record<string, string>,
          { g: 'g option' } as Record<string, string>,
          { h: 'h option' } as Record<string, string>,
          { i: 'i option' } as Record<string, string>,
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
            },
          ],
        ],
        name: 'Mock Setting Nested Array Name',
        description: 'Mock setting nested array description',
      },
    ],
  },

  modSettings: {
    'mock-setting': 'mock-setting-value',
    'mock-setting-dropdown': 'mock-setting-value',
    'mock-setting-array[0]': 'a',
    'mock-setting-array[1]': 'b',
    'mock-setting-array[2]': 'c',
  },

  modVersions: [
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
  ],

  modVersionSource: (version: string) => ({
    source: `// Mock source for version ${version}...\n`,
    metadata: {
      ...mockModMetadata,
      version,
    },
    readme: `# Mock readme for version ${version}...\n`,
    initialSettings: [],
  }),

  modConfig: {
    'custom-message-box': mockModConfig,
    'local@asdf2': mockModConfig,
    asdf3: mockModConfig,
    asdf4: mockModConfig,
    asdf5: mockModConfig,
    asdf6: mockModConfig,
    asdf7: mockModConfig,
  },

  // ============================================================================
  // Sidebar (editor mode)
  // ============================================================================

  sidebarModDetails: {
    modId: 'new-mod-test',
    modDetails: mockModConfig,
    modWasModified: false,
    noWindhawkExitButton: false,
  },
};
