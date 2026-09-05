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
  type InstalledModDetails,
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
// The repository side of a listed mod, and only that: whether one is on the
// machine is answered by `installedMods`, which `repositoryModsListing` joins in
// when the listing is served. A fixture that could carry an installed side of its
// own would be describing one machine twice, which is how the two browsers came
// to disagree about the same mod.
export type RepositoryModType = Omit<
  NonNullable<GetRepositoryModsReplyData['mods']>[string],
  'installed'
>;
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
  // The version a source names, which is what an install reports it put on the
  // machine - read out of the source, not assumed to be the one on offer.
  modVersionOfSource: (modSource: string) => string | undefined;
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
  author: 'John Smith',
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
  updatesDisabledForVersion: '',
};

// A mod with nothing waiting for it, which is what naming no repository version
// says: a version differing from the installed one IS the offer.
const mockModDetails: ModDetailsType = {
  metadata: {},
  config: mockModConfig,
  latestVersion: null,
  userRating: 0,
};

// A mod the host has found an update for, at the version the repository fixture
// hands out for a mod of no particular id. Several of them, so the batch update
// flow has a list with a middle rather than a single row.
const mockModDetailsUpdatable: ModDetailsType = {
  ...mockModDetails,
  latestVersion: mockModMetadataOnline.version ?? null,
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

// The filler the browser's batches, ranking, search and filters are read for:
// what those screens are about is the list rather than any one mod, so these
// carry the least a card needs and none of them is on the machine.
const mockNumberedRepositoryMods: Record<string, RepositoryModType> =
  Object.fromEntries(
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
  );

// The mod the home screen features, which the strip only shows while it is not
// on the machine - so it is one of the numbered mods rather than the sample,
// which is installed.
const FEATURED_MOD_ID = 'online050';

const mockRepositoryMods: Record<string, RepositoryModType> = {
  // The sample mod, listed at the version an update of it would bring: it is the
  // one repository mod the machine has, under the same id `installedMods`
  // reports it by, which is what puts one mod in front of both browsers.
  'custom-message-box': {
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
  },
  ...mockNumberedRepositoryMods,
};

// The repository listing as a host answers it: the fixtures' repository side,
// with the installed side joined in for every listed mod the machine has. The
// machine is described once, by `installedMods`, so a state a test sets up there
// reaches the repository browser as well as the home screen.
export function repositoryModsListing(
  mockData: MockDataRegistry
): NonNullable<GetRepositoryModsReplyData['mods']> {
  return Object.fromEntries(
    Object.entries(mockData.repositoryMods).map(([modId, mod]) => {
      const installed = mockData.installedMods[modId];
      if (!installed) {
        return [modId, mod];
      }
      // Named field by field rather than spread: the listing carries the mod and
      // the version the machine last cached for it, which is what both hosts join
      // in here, and nothing else an installed entry happens to hold.
      return [
        modId,
        {
          ...mod,
          installed: {
            metadata: installed.metadata,
            config: installed.config,
            userRating: installed.userRating,
            latestVersion: installed.latestVersion,
          },
        },
      ];
    })
  );
}

// The details an install or a recompile replies with, as a host answers them: what
// the operation put on the machine, over the profile-held fields the listing taken
// after it would name. The repository version stands as the machine last cached
// it - an install does not go and look - so installing a version other than the
// one it names leaves the offer standing, and a screen reading the two says so.
export function installedModDetailsAfterOperation(
  mockData: MockDataRegistry,
  modId: string,
  metadata: ModMetadata,
  config: ModConfig
): InstalledModDetails {
  const installed = mockData.installedMods[modId];
  return {
    metadata,
    config,
    latestVersion: installed?.latestVersion ?? null,
    userRating: installed?.userRating ?? 0,
  };
}

// The events a host pushes of its own accord once it has answered a command,
// which are how a change reaches the screens that did not make it - and the one
// that did: a reply says the write was taken, the event says what the mod now
// is. A screen following only its own replies would go on showing the config it
// asked to change.
export function hostEventsAfterReply(
  command: string,
  request: Record<string, unknown>,
  reply: Record<string, unknown>
): Array<{ command: string; data: Record<string, unknown> }> {
  // The echo of a config write, carrying the patch that was written - which is a
  // config only over the one it was written against. Only for a write the host
  // took: a refused one changed nothing to tell anybody about.
  if (command === 'updateModConfig' && reply['succeeded']) {
    return [
      {
        command: 'setNewModConfig',
        data: { modId: request['modId'], config: request['config'] },
      },
    ];
  }
  return [];
}

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
    disableToolkitHotkey: false,
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
      latestVersion: mockModMetadataOnline.version ?? null,
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
      latestVersion: mockModMetadataLargeOnline.version ?? null,
      userRating: 0,
    },
  },

  featuredMods: {
    [FEATURED_MOD_ID]: mockNumberedRepositoryMods[FEATURED_MOD_ID].repository,
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

  // Read back out of the text, as a host reads it out of a mod's header. Absent
  // for a source naming none, which falls back to the version on offer.
  modVersionOfSource: (modSource: string) =>
    /^\/\/ Mock source of .*, version (.+?)\.\.\.$/m.exec(modSource)?.[1],

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
    appSettings: { requiresRestart: true },
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
