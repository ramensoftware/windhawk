import {
  largeModSourceInstalled,
  largeModSourceRepository,
} from './largeModSource';
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
  type UserDataImportSummary,
  type UserDataManifest,
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
  // The installed side of a mod. Keyed by mod like the repository side below, so
  // that one mod can be described differently from the rest.
  installedModSourceData: (modId: string) => InstalledModSourceData;
  modSettings: Record<string, unknown>;
  modVersions: ModVersion[];
  // The repository side of a mod: its source at the given version, or at the
  // version the repository currently offers when none is asked for.
  modVersionSource: (modId: string, version?: string) => InstalledModSourceData;
  modConfig: Record<string, ModConfig>;
  // The config a mod carries once it has been installed or compiled.
  newModConfig: ModConfig;

  // User-data export/import
  userDataManifest: UserDataManifest;
  userDataArchive: string;
  userDataImportSummary: UserDataImportSummary;

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

// The mod that stands for a real one's size. Every other mod's source here is a
// line long, which shows that the source and diff screens render but nothing
// about what they cost on a mod of a few thousand lines. The id is the one its
// generated source declares, and it is installed with an update waiting, which is
// what puts a diff of it in front of the user - the Changes tab and the update
// wizard's per-mod detail.
//
// It is deliberately not in the repository listing: the mods it would sit among
// are there to fill the browser's batches and its ranking, and one more heavy
// entry would only slow the screens it is not meant to say anything about.
const LARGE_MOD_ID = 'large-diff-sample';

// The versions match the rest of the fixtures rather than the mod's own history,
// so a wizard row reads the way every other row does.
const mockModMetadataLarge: ModMetadata = {
  name: 'Large Diff Sample',
  description: 'A mod large enough to measure the diff against',
  version: '0.1',
  author: 'Mock',
  github: 'https://github.com/mock',
  include: ['*'],
};

const mockModMetadataLargeOnline: ModMetadata = {
  ...mockModMetadataLarge,
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

// A mod the host has found an update for. Several of them, so the batch update
// flow has a list with a middle rather than a single row.
const mockModDetailsUpdatable: ModDetailsType = {
  ...mockModDetails,
  updateAvailable: true,
};

const mockReadme = `# Mock readme...

| Month    | Savings |
| -------- | ------- |
| January  | $250    |
| February | $80     |
| March    | $420    |

More text...`;

// One setting of each shape the settings editor renders: a plain string, a
// string whose declared default is too long to show whole, a dropdown, an array,
// an array of nested objects, and a nested object.
const mockInitialSettings: InitialSettings = [
  {
    key: 'mock-setting',
    value: 'mock-setting-value',
    name: 'Mock Setting Name',
    description: 'Mock setting description',
  },
  {
    key: 'mock-setting-long-default',
    value:
      'A default long enough that naming it beside the setting has to be cut short to fit',
    name: 'Mock Setting Long Default Name',
    description: 'Mock setting long default description',
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
  {
    key: 'mock-setting-nested-object',
    value: [
      {
        key: 'mock-setting-nested-object-child',
        value: 'mock-setting-nested-object-child-value',
        name: 'Mock Setting Nested Object Child Name',
        description: 'Mock setting nested object child description',
      },
    ],
    name: 'Mock Setting Nested Object Name',
    description: 'Mock setting nested object description',
  },
];

const mockRepositoryMods: Record<string, RepositoryModType> = {
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
    asdf3: mockModDetailsUpdatable,
    asdf4: mockModDetails,
    asdf5: mockModDetailsUpdatable,
    asdf6: mockModDetails,
    asdf7: mockModDetails,
    [LARGE_MOD_ID]: {
      metadata: mockModMetadataLarge,
      config: mockModConfig,
      updateAvailable: true,
      userRating: 0,
    },
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

  repositoryMods: mockRepositoryMods,

  // ============================================================================
  // Mod details
  // ============================================================================

  // The source text carries the mod so a diff against the repository side has
  // something to show; the large mod carries a whole one instead, so the diff has
  // the size a real one does.
  installedModSourceData: (modId: string) => ({
    source:
      modId === LARGE_MOD_ID
        ? largeModSourceInstalled
        : '// Mock local source...\n',
    metadata: modId === LARGE_MOD_ID ? mockModMetadataLarge : mockModMetadata,
    readme: mockReadme,
    initialSettings: mockInitialSettings,
  }),

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

  // A repository mod is described by its own entry when the repository lists it,
  // and by the online flavor of the sample mod otherwise (the installed mods are
  // not all in the mock repository). The source text carries the mod and version
  // so a diff against the installed source has something to show.
  modVersionSource: (modId: string, version?: string) => {
    const metadata =
      modId === LARGE_MOD_ID
        ? mockModMetadataLargeOnline
        : mockRepositoryMods[modId]?.repository.metadata ?? mockModMetadataOnline;
    const resolvedVersion = version ?? metadata.version;
    return {
      source:
        modId === LARGE_MOD_ID
          ? largeModSourceRepository
          : `// Mock source of ${modId}, version ${resolvedVersion}...\n`,
      metadata: { ...metadata, version: resolvedVersion },
      readme: mockReadme,
      initialSettings: mockInitialSettings,
    };
  },

  modConfig: {
    'custom-message-box': mockModConfig,
    [LARGE_MOD_ID]: mockModConfig,
    'local@asdf2': mockModConfig,
    asdf3: mockModConfig,
    asdf4: mockModConfig,
    asdf5: mockModConfig,
    asdf6: mockModConfig,
    asdf7: mockModConfig,
  },

  newModConfig: mockModConfig,

  // ============================================================================
  // User-data export/import
  // ============================================================================

  // The manifest a mock inspect projects, so the Import dialog opens over a
  // realistic archive in development mode. A reference-only repository mod
  // (hasSource: false), a local mod (source always embedded), and a mod carrying
  // neither facet, to exercise the "not in this archive" states.
  userDataManifest: {
    exportedAt: '2025-01-15T10:30:00Z',
    hasAppSettings: true,
    mods: [
      {
        modId: 'custom-message-box',
        isLocal: false,
        version: '0.1',
        name: 'Custom Message Box',
        hasSource: false,
        hasSettings: true,
        hasConfig: true,
      },
      {
        modId: 'local@asdf2',
        isLocal: true,
        version: '1.0',
        name: null,
        hasSource: true,
        hasSettings: true,
        hasConfig: true,
      },
      {
        modId: 'asdf3',
        isLocal: false,
        version: '1.0',
        name: null,
        hasSource: false,
        hasSettings: false,
        hasConfig: false,
      },
    ],
  },

  userDataArchive: '{\n  "format": "windhawk-user-data-v1"\n}',

  userDataImportSummary: {
    mods: [
      { modId: 'custom-message-box', status: 'installed' },
      {
        modId: 'local@asdf2',
        status: 'skipped',
        message: 'already installed (--on-conflict skip)',
      },
      { modId: 'asdf3', status: 'failed', message: 'Compilation failed' },
    ],
    appSettings: { requiresRestart: true, requiresNotify: false },
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
