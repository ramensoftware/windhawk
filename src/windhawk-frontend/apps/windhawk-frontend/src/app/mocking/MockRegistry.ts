import {
  largeModSourceInstalled,
  largeModSourceRepository,
} from './largeModSource';
import type {
  ReviewPostFields,
  ModReviewsDocument,
} from '@app/panel/shared/reviews/modReviews';
import {
  type AppSettings,
  type AppUISettings,
  type CurrentUser,
  type DynamicSelectOption,
  type GetFeaturedModsReplyData,
  type GetInstalledModsReplyData,
  type GetModVersionsReplyData,
  type GetRepositoryModsReplyData,
  type HotkeyCaptureModifiers,
  type InitialSettings,
  type InstalledModDetails,
  type ModReviewVote,
  type ModConfig,
  type ModMetadata,
  type SetEditedModDetailsData,
  type UserDataImportSummary,
  type UserDataManifest,
  type UserScope,
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
  // The account the host acts for, which a mod's user configuration belongs to.
  currentUser: CurrentUser;

  // Mods browser - Local
  installedMods: Record<string, ModDetailsType>;
  featuredMods: Record<string, FeaturedModDetailsType>;

  // Mods browser - Online
  repositoryMods: Record<string, RepositoryModType>;

  // Mod details
  // The installed side of a mod. Keyed by mod like the repository side below, so
  // that one mod can be described differently from the rest.
  installedModSourceData: (modId: string) => InstalledModSourceData;
  // The store's values for a mod's settings, keyed by mod like the source data
  // above: the keys follow the block the mod declares.
  modSettings: (modId: string) => Record<string, string | number>;
  // The settings a mod was saved with, keyed by mod, which a read answers with
  // over `modSettings`. A save replaces the whole map, as the store's does.
  savedModSettings: Record<string, Record<string, string | number>>;
  // What a mod wrote at runtime for its `$dynamicSelect` settings, keyed by
  // setting path. Served for every mod: a path no mod declares is never asked
  // for.
  modDynamicSelectOptions: Record<string, DynamicSelectOption[]>;
  // The file the host's Open dialog picks, and the folder its folder dialog
  // picks, whichever setting asked.
  pickedFilePath: string;
  pickedFolderPath: string;
  // The font families installed on the host machine.
  fontFamilies: string[];
  // What the host records when a hotkey setting's badge asks for a shortcut:
  // the modifiers it reports held on the way, one progress event each spread
  // over the wait, then the shortcut in the stored form - after a wait, as a
  // person takes one to press a shortcut, so the badge's recording states are
  // there to be seen.
  hotkeyCapture: {
    heldOnTheWay: HotkeyCaptureModifiers[];
    hotkey: string;
    delayMs: number;
  };
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

  // Mod reviews
  // A mod's reviews document as the pages site serves it, or null for a mod
  // with no file, which reads as no reviews.
  modReviews: (modId: string) => ModReviewsDocument | null;
  // A post, answered as the update server answers one: with the id it landed
  // under, pending approval.
  postModReview: (fields: ReviewPostFields) => Promise<{ id: number }>;
  // The votes the user cast, keyed by mod, as the host reads them out of the
  // profile. A vote cast in mock mode is written back here and one taken back
  // removed, so a reopened modal finds what the host would hold.
  reviewVotes: Record<string, ModReviewVote[]>;
  // Whether the reviews document and the post go out over the network rather
  // than being answered here. Off, mock mode stands in for the server as it
  // does for the host; on, the requests go out as the app would send them.
  commentsOverNetwork: boolean;

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

// The mod whose settings block is mockAnnotatedInitialSettings, so the browser
// preview has each annotation's control on one page. Named to sort after the
// other mods, leaving their order as it was; not in the repository listing and
// with nothing waiting for it, the settings tab being what it is about.
const ANNOTATED_MOD_ID = 'settings-annotations-sample';

const mockModMetadataAnnotated: ModMetadata = {
  name: 'Settings Annotations Sample',
  description: 'A mod whose settings use every annotation',
  version: '1.0',
  author: 'Mock',
  github: 'https://github.com/mock',
  include: ['*'],
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
  userScope: 'none',
  nuked: '',
};

// What the catalog says about a mod it flags: a line with a web address in it,
// the shape the repository publishes.
export const mockNukedReason =
  'Causes crashes, for details see https://windhawk.net/2026/x-y-crashes';

// The mod the Windhawk mod catalog took off the machine, with nothing on offer
// to restore it. It was running when taken off, so its switch says it is on.
// Not in the repository listing, for the reason the large mod is not.
const NUKED_MOD_ID = 'nuked-sample';

const mockModMetadataNuked: ModMetadata = {
  name: 'Nuked Sample',
  description: 'A mod the Windhawk mod catalog disabled',
  version: '1.0',
  author: 'Mock',
  github: 'https://github.com/mock',
  include: ['*'],
};

const mockModConfigNuked: ModConfig = {
  ...mockModConfig,
  nuked: mockNukedReason,
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

// The last line links another mod's page on windhawk.net, which the app draws
// as the mod, and the site itself, which it leaves as written. On that line
// rather than one of its own, so the details page still fits the viewport.
const mockReadme = `# Mock readme...

| Month    | Savings |
| -------- | ------- |
| January  | $250    |
| February | $80     |
| March    | $420    |

More text, on [My Mod 002](https://windhawk.net/mods/online002) and the [Windhawk website](https://windhawk.net/)...`;

// The same readme under a page's worth of text, which puts its link to the other
// mod well below the viewport: for where on a page a reader comes back to. A
// scenario's, never the default's, for the reason above.
export const mockLongReadme = [
  ...Array.from(
    { length: 60 },
    (_, index) => `Paragraph ${index + 1} of a readme long enough to scroll.`
  ),
  mockReadme,
].join('\n\n');

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

// The settings block of a mod using the data-type annotations, one setting per
// shape the docs give them (windhawk.wiki, "Settings annotations added in
// Windhawk 2.0"); each setting's description names the shape it stands for.
// The block of the annotated sample mod on the default machine, and of every
// mod under the annotated-settings scenario (see mockScenarios).
export const mockAnnotatedInitialSettings: InitialSettings = [
  {
    key: 'accentColor',
    value: '#3399FF',
    name: 'Accent color',
    description: 'A color without an alpha channel',
    format: 'colorRgb',
  },
  {
    key: 'overlayColor',
    value: '#80FFFFFF',
    name: 'Overlay color',
    description: 'A color with its alpha first',
    format: 'colorArgb',
  },
  {
    key: 'soundFile',
    value: '',
    name: 'Sound file',
    description: 'A file the host picks',
    format: 'filePath',
  },
  {
    key: 'opacity',
    value: '0.85',
    name: 'Opacity',
    description: 'A decimal number on a slider between 0 and 1',
    float: true,
    min: 0,
    max: 1,
    format: 'slider',
  },
  {
    key: 'scale',
    value: '1',
    name: 'Scale factor',
    description: 'A decimal number declared as an integer',
    float: true,
  },
  {
    key: 'weights',
    value: ['0.25', '0.5', '1'],
    name: 'Weights',
    description: 'An array of decimal numbers',
    float: true,
  },
  {
    key: 'outputDevice',
    value: '',
    name: 'Output device',
    description: 'A dropdown the mod fills at runtime',
    dynamicSelect: true,
  },
  {
    key: 'monitor',
    value: 'primary',
    name: 'Monitor',
    description: 'A declared option above a runtime list',
    options: [{ primary: 'Primary monitor' } as Record<string, string>],
    dynamicSelect: true,
  },
  {
    key: 'rules',
    value: [
      [
        {
          key: 'device',
          value: '',
          name: 'Device',
          description: 'A runtime dropdown every row shares',
          dynamicSelect: true,
        },
        { key: 'label', value: '', name: 'Label' },
      ],
    ],
    name: 'Rules',
    description: 'Rows sharing one runtime dropdown',
  },
  {
    key: 'fontName',
    value: 'Segoe UI',
    name: 'Font',
    description: 'A font family, completed over the installed ones',
    format: 'fontFamily',
  },
  {
    key: 'logFolder',
    value: '',
    name: 'Log folder',
    description: 'A folder the host picks',
    format: 'folderPath',
  },
  {
    key: 'toggleHotkey',
    value: 'ctrl+alt+84',
    name: 'Toggle hotkey',
    description: 'A keyboard chord, recorded from the keys pressed',
    format: 'hotkey',
  },
  {
    key: 'highlightColors',
    value: ['#FF8800', '#00AAFF'],
    name: 'Highlight colors',
    description: 'A color per element of a string array',
    format: 'colorRgb',
  },
  {
    key: 'homepageUrl',
    value: '',
    name: 'Homepage',
    description: 'A format the editor does not know, drawn as plain text',
    format: 'url',
  },
  {
    key: 'rating',
    value: 3,
    name: 'Rating',
    description: 'An integer held between 1 and 5',
    min: 1,
    max: 5,
  },
  {
    key: 'zoom',
    value: '1.5',
    name: 'Zoom',
    description: 'A decimal held between 0.5 and 4',
    float: true,
    min: 0.5,
    max: 4,
  },
  {
    key: 'iconSize',
    value: 24,
    name: 'Icon size',
    description: 'An integer on a slider between 16 and 64',
    min: 16,
    max: 64,
    format: 'slider',
  },
  {
    key: 'retries',
    value: 3,
    name: 'Retries',
    description: 'An integer with only a lower bound, which a slider cannot span',
    min: 0,
    format: 'slider',
  },
  {
    key: 'columnWidths',
    value: [120, 80, 200],
    name: 'Column widths',
    description: 'Integers each on a slider between 40 and 400',
    min: 40,
    max: 400,
    format: 'slider',
  },
  {
    key: 'watchedDevices',
    value: [''],
    name: 'Watched devices',
    description: 'A runtime dropdown per element of a string array',
    dynamicSelect: true,
  },
  {
    key: 'fadeEnabled',
    value: false,
    name: 'Fade trick',
    description: 'A switch, gating the settings after it',
  },
  {
    key: 'fadeDelay',
    value: 100,
    name: 'Fade delay',
    description: 'Shown while the fade trick is on',
    showIf: { fadeEnabled: [true] },
  },
  {
    key: 'centerOffset',
    value: 0,
    name: 'Center offset',
    description: 'Hidden while the fade trick is on',
    hideIf: { fadeEnabled: [true] },
  },
  {
    key: 'fadeExclusions',
    value: [''],
    name: 'Fade exclusions',
    description: 'An array shown while the fade trick is on',
    showIf: { fadeEnabled: [true] },
  },
  {
    key: 'notifyMode',
    value: 'none',
    name: 'Notification',
    description: 'A dropdown, gating the setting after it and the badge sound',
    options: [
      { none: 'None' } as Record<string, string>,
      { toast: 'Toast' } as Record<string, string>,
      { custom: 'Custom text' } as Record<string, string>,
    ],
  },
  {
    key: 'notifyText',
    value: '',
    name: 'Notification text',
    description: 'Shown for the custom notification',
    showIf: { notifyMode: ['custom'] },
  },
  {
    key: 'actions',
    value: [
      [
        {
          key: 'action',
          value: 'nothing',
          name: 'Action',
          options: [
            { nothing: 'Nothing' } as Record<string, string>,
            { keypress: 'Key press' } as Record<string, string>,
            { start: 'Start a program' } as Record<string, string>,
          ],
        },
        {
          key: 'args',
          value: '',
          name: 'Arguments',
          description: 'Shown for a key press or a program, per row',
          showIf: { 'actions.action': ['keypress', 'start'] },
        },
      ],
    ],
    name: 'Actions',
    description: 'Rows each gating a field on their own action',
  },
  {
    key: 'showBadge',
    value: false,
    name: 'Badge',
    description: 'A switch gating the two settings after it, each beside another gate',
  },
  {
    key: 'badgeFadeDelay',
    value: 200,
    name: 'Badge fade delay',
    description: 'Shown while both the badge and the fade trick are on',
    showIf: { showBadge: [true], fadeEnabled: [true] },
  },
  {
    key: 'badgeSound',
    value: false,
    name: 'Badge sound',
    description: 'Shown while the badge is on, unless the notification is off',
    showIf: { showBadge: [true] },
    hideIf: { notifyMode: ['none'] },
  },
  {
    key: 'tabWidth',
    value: 0,
    name: 'Tab width',
    description: 'An integer with only an upper bound; 0 sizes each tab to its title',
    max: 600,
  },
  {
    key: 'tabAlignment',
    value: 'left',
    name: 'Tab title alignment',
    description: 'Hidden while the tab width is 0, the integer for automatic',
    options: [
      { left: 'Left' } as Record<string, string>,
      { center: 'Center' } as Record<string, string>,
      { right: 'Right' } as Record<string, string>,
    ],
    hideIf: { tabWidth: [0] },
  },
  {
    key: 'rendering',
    value: [
      {
        key: 'useVisualStyles',
        value: true,
        name: 'Use visual styles',
        description: 'A switch the group after this one names by a dotted path',
      },
    ],
    name: 'Rendering',
    description: 'A group whose switch gates the group after it',
  },
  {
    key: 'customRendering',
    value: [
      {
        key: 'renderBorder',
        value: true,
        name: 'Draw a border',
        description: 'A switch its sibling group names by an outward lookup',
      },
      {
        key: 'lightModeColors',
        value: [
          {
            key: 'borderColor',
            value: '#BCBCBC',
            name: 'Border color',
            format: 'colorRgb',
          },
        ],
        name: 'Light mode colors',
        description: 'A group shown while the border is drawn',
        showIf: { 'customRendering.renderBorder': [true] },
      },
    ],
    name: 'Custom rendering',
    description: 'A group shown while visual styles are off',
    showIf: { 'rendering.useVisualStyles': [false] },
  },
];

// What the store holds for mockAnnotatedInitialSettings, in the spellings the
// controls have to read right: a color typed without the `#` in lowercase, a decimal
// held as the integer a DWORD reads back as, one spelled with a trailing zero,
// a dropdown value neither the declaration nor the runtime list names, a
// hotkey in the stored form, bounded numbers within their bounds, and the
// gates at the values that hide their dependents.
export const mockAnnotatedModSettings: Record<string, string | number> = {
  accentColor: '3399ff',
  overlayColor: '#80FFFFFF',
  opacity: 1,
  scale: '1.0',
  'weights[0]': '0.25',
  'weights[1]': '0.5',
  'weights[2]': '1',
  outputDevice: '{0.0.0.00000000}.{a1b2c3d4-0000-0000-0000-00000000dead}',
  monitor: 'primary',
  fontName: 'Segoe UI',
  toggleHotkey: 'ctrl+alt+84',
  'highlightColors[0]': 'ff8800',
  'highlightColors[1]': '#00AAFF',
  homepageUrl: 'https://example.com/',
  rating: 3,
  zoom: '1.5',
  iconSize: 24,
  retries: 3,
  'columnWidths[0]': 120,
  'columnWidths[1]': 80,
  'columnWidths[2]': 200,
  'watchedDevices[0]': 'usb::046d:c52b',
  fadeEnabled: 0,
  fadeDelay: 100,
  centerOffset: 0,
  'fadeExclusions[0]': '',
  notifyMode: 'none',
  notifyText: '',
  'actions[0].action': 'nothing',
  'actions[0].args': '',
  showBadge: 0,
  badgeFadeDelay: 200,
  badgeSound: 0,
  tabWidth: 0,
  tabAlignment: 'left',
  'rendering.useVisualStyles': 1,
  'customRendering.renderBorder': 1,
  'customRendering.lightModeColors.borderColor': '#BCBCBC',
};

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
              trendingSorting: 1,
              published: 1618321977408,
              updated: 1718321977408,
              reviews: 0,
            },
          },
        },
      ])
  );

// The mod the home screen features, which the strip only shows while it is not
// on the machine - so it is one of the numbered mods rather than the sample,
// which is installed. It stands out of the filler by its numbers: enough users
// for the card to draw a compact count, and a reviews document of its own.
const FEATURED_MOD_ID = 'online050';
mockNumberedRepositoryMods[FEATURED_MOD_ID] = {
  repository: {
    ...mockNumberedRepositoryMods[FEATURED_MOD_ID].repository,
    details: {
      ...mockNumberedRepositoryMods[FEATURED_MOD_ID].repository.details,
      users: 12345,
      reviews: 2,
    },
  },
};

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
        trendingSorting: 0,
        published: 1618321977408,
        updated: 1718321977408,
        reviews: 4,
      },
    },
  },
  ...mockNumberedRepositoryMods,
};

// The reviews of the two mods that have any: the sample mod, which both
// browsers list and the machine has (so the header's reviews icon and the
// card's popover open the same document), and the featured mod. The sample
// mod's has one of each thing the modal draws: replies under a review, a
// review the user voted on that the server counts at zero, a review in a
// right-to-left script, and one long enough to wrap, written in markdown with
// line breaks of its own and an image the modal does not draw.
const mockModReviews: Record<string, ModReviewsDocument> = {
  'custom-message-box': {
    modId: 'custom-message-box',
    reviews: [
      {
        id: 101,
        parentId: null,
        timestamp: 1757800000,
        authorName: 'Jane',
        modVersion: '0.1',
        content: 'Works great on 24H2.\nThe seconds option is what I was after.',
        votes: 3,
      },
      {
        id: 102,
        parentId: null,
        timestamp: 1757850000,
        authorName: 'Bob',
        modVersion: '0.2',
        content: 'Broke after a Windows update; reinstalling it fixed things.',
        votes: 0,
      },
      {
        id: 103,
        parentId: null,
        timestamp: 1757900000,
        authorName: 'דנה',
        modVersion: '0.2',
        content: 'עובד מצוין, בדיוק מה שחיפשתי.',
        votes: 1,
      },
      {
        id: 104,
        parentId: null,
        timestamp: 1757950000,
        authorName: 'Larry',
        modVersion: null,
        content: [
          'A longer review, the kind that needs a few paragraphs.',
          '',
          'The first thing to say is that the mod **does what it says**, and does it on every monitor I tried it on, including one scaled to 175%.',
          'The second is that the settings page could use a description or two, but the defaults are sensible enough that most people will never open it.',
          '',
          'Tried on:',
          '',
          '- a 4K monitor at 150%',
          '- a laptop screen at 175%',
          '',
          '![The seconds option](https://example.com/seconds.png)',
          '',
          'Recommended.',
        ].join('\n'),
        votes: 2,
      },
      {
        id: 105,
        parentId: 101,
        timestamp: 1757860000,
        authorName: 'Bob',
        modVersion: null,
        content: 'Same here.',
        votes: 0,
      },
      {
        id: 106,
        parentId: 101,
        timestamp: 1757870000,
        authorName: 'Jane',
        modVersion: '0.1',
        content: 'Glad it helped.',
        votes: 1,
      },
    ],
  },
  [FEATURED_MOD_ID]: {
    modId: FEATURED_MOD_ID,
    reviews: [
      {
        id: 201,
        parentId: null,
        timestamp: 1758000000,
        authorName: 'Ada',
        modVersion: '1.2',
        content: 'Does exactly one thing, and does it well.',
        votes: 5,
      },
      {
        id: 202,
        parentId: null,
        timestamp: 1758100000,
        authorName: 'Grace',
        modVersion: '1.2',
        content: 'Would like an option to skip the first monitor.',
        votes: 1,
      },
      {
        id: 203,
        parentId: 202,
        timestamp: 1758150000,
        authorName: 'John Smith',
        modVersion: null,
        content: 'Planned for the next version.',
        votes: 0,
      },
    ],
  },
};

// Where a mock post's ids start: past every id a document above uses, as the
// server's counter would be.
let nextMockReviewId = 1000;

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

// What a host answers a change of a mod's userScope with: the config as it reads
// afterwards. The registry takes it in writeMockReply.
export function mockUserScopeReply(
  mockData: MockDataRegistry,
  modId: string,
  userScope: (current: UserScope) => UserScope
) {
  const config = mockData.modConfig[modId];
  if (!config) {
    return {
      modId,
      succeeded: false,
      error: {
        code: 'MOD_NOT_INSTALLED',
        message: `Mod ${modId} is not installed`,
      },
    };
  }
  return {
    modId,
    succeeded: true,
    config: { ...config, userScope: userScope(config.userScope) },
  };
}

// `slice` with `key` holding `value`, or nothing for undefined. A write replaces
// the slice rather than editing it: a read hands out the slice itself, which a
// screen may still be holding.
function withKey<T>(
  slice: Record<string, T>,
  key: string,
  value: T | undefined
): Record<string, T> {
  const next = { ...slice };
  if (value === undefined) {
    delete next[key];
  } else {
    next[key] = value;
  }
  return next;
}

// A change to a mod's config, made wherever the registry holds one: the config
// the details read, and the listing's entry. A mod listed without a config has
// none to change.
function patchModConfig(
  mockData: MockDataRegistry,
  modId: string,
  patch: Partial<ModConfig>
) {
  const config = mockData.modConfig[modId];
  if (config) {
    mockData.modConfig = withKey(mockData.modConfig, modId, {
      ...config,
      ...patch,
    });
  }
  const listed = mockData.installedMods[modId];
  if (listed?.config) {
    mockData.installedMods = withKey(mockData.installedMods, modId, {
      ...listed,
      config: { ...listed.config, ...patch },
    });
  }
}

// The fields of the app UI settings a host reads off the app settings; the
// update availability comes from elsewhere.
function appUISettingsOf(settings: AppSettings): Partial<AppUISettings> {
  return {
    language: settings.language,
    devModeOptOut: settings.devModeOptOut,
    loggingEnabled:
      settings.loggingVerbosity > 0 || settings.engine.loggingVerbosity > 0,
    safeMode: settings.safeMode,
    ...(settings.theme !== undefined && { theme: settings.theme }),
  };
}

// What a reply says the host wrote, written into the registry so a read that
// follows finds it, as on a host. Taken from the reply as the active scenario
// left it, so a write it turned into a failure leaves nothing behind, and from
// the request where only the request carries what was written.
export function writeMockReply(
  command: string,
  mockData: MockDataRegistry,
  request: Record<string, unknown>,
  reply: Record<string, unknown>
) {
  const modId = reply['modId'] as string;
  // An install or a recompile answers with the mod it left on the machine, and
  // a failed one with none.
  if (command === 'installMod' || command === 'compileMod') {
    const details = reply[
      command === 'installMod' ? 'installedModDetails' : 'compiledModDetails'
    ] as InstalledModDetails | null;
    if (details) {
      mockData.installedMods = withKey(mockData.installedMods, modId, details);
      mockData.modConfig = withKey(mockData.modConfig, modId, details.config);
    }
    return;
  }
  if (!reply['succeeded']) {
    return;
  }
  switch (command) {
    case 'updateModConfig':
      patchModConfig(mockData, modId, request['config'] as Partial<ModConfig>);
      break;
    case 'setModUserScope':
    case 'deleteModUserConfig':
      patchModConfig(mockData, modId, reply['config'] as ModConfig);
      break;
    case 'enableMod':
      patchModConfig(mockData, modId, { disabled: !reply['enabled'] });
      break;
    case 'updateModRating': {
      const listed = mockData.installedMods[modId];
      if (listed) {
        mockData.installedMods = withKey(mockData.installedMods, modId, {
          ...listed,
          userRating: reply['rating'] as number,
        });
      }
      break;
    }
    // Its config and its settings go with the mod.
    case 'deleteMod':
      mockData.installedMods = withKey(mockData.installedMods, modId, undefined);
      mockData.modConfig = withKey(mockData.modConfig, modId, undefined);
      mockData.savedModSettings = withKey(
        mockData.savedModSettings,
        modId,
        undefined
      );
      break;
    case 'setModSettings':
      mockData.savedModSettings = withKey(
        mockData.savedModSettings,
        modId,
        request['settings'] as Record<string, string | number>
      );
      break;
    case 'voteModReview':
    case 'retractModReviewVote':
      mockData.reviewVotes = withKey(
        mockData.reviewVotes,
        modId,
        reply['votes'] as ModReviewVote[]
      );
      break;
    // A patch, applied field by field as the core applies one, the engine's
    // fields included.
    case 'updateAppSettings': {
      const patch = reply['appSettings'] as Partial<AppSettings>;
      const settings = mockData.appSettings;
      mockData.appSettings = {
        ...settings,
        ...patch,
        engine: { ...settings.engine, ...patch.engine },
      };
      mockData.appUISettings = {
        ...mockData.appUISettings,
        ...appUISettingsOf(mockData.appSettings),
      };
      break;
    }
  }
}

// The events a host pushes while it works on a command, ahead of its answer,
// spread over the wait the answer takes. Only a capture that ran pushes them:
// one the host refused (the reply carries an error) recorded nothing on the way.
export function hostEventsBeforeReply(
  command: string,
  mockData: MockDataRegistry,
  reply: Record<string, unknown>
): Array<{ command: string; data: Record<string, unknown> }> {
  if (command === 'captureHotkey' && reply['error'] === undefined) {
    return mockData.hotkeyCapture.heldOnTheWay.map((modifiers) => ({
      command: 'hotkeyCaptureProgress',
      data: { modifiers },
    }));
  }
  return [];
}

// How long the mock host takes to answer a command: at once, but for a capture,
// which stands for the user pressing a shortcut.
export function mockReplyDelayMs(command: string, mockData: MockDataRegistry): number {
  return command === 'captureHotkey' ? mockData.hotkeyCapture.delayMs : 0;
}

// The events a host pushes as it answers a command, just ahead of the answer,
// which are how a change reaches the screens that did not make it - and the one
// that did: a reply says the write was taken, the event says what the mod now
// is, and a caller awaiting the reply finds it already applied.
export function hostEventsWithReply(
  command: string,
  mockData: MockDataRegistry,
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
  // A scope switch or a delete can change every field the user sees, so its
  // echo is the whole config the reply carries.
  if (
    (command === 'setModUserScope' || command === 'deleteModUserConfig') &&
    reply['succeeded']
  ) {
    return [
      {
        command: 'setNewModConfig',
        data: { modId: request['modId'], config: reply['config'] },
      },
    ];
  }
  // The app UI settings as the write left them, which is how a language or an
  // opt-out reaches the app.
  if (command === 'updateAppSettings' && reply['succeeded']) {
    return [
      {
        command: 'setNewAppSettings',
        data: { appUISettings: mockData.appUISettings },
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

  currentUser: {
    sid: 'S-1-5-21-1004336348-1177238915-682003330-1001',
    name: 'DESKTOP-MOCK\\user',
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
    [ANNOTATED_MOD_ID]: {
      metadata: mockModMetadataAnnotated,
      config: mockModConfig,
      latestVersion: null,
      userRating: 0,
    },
    [NUKED_MOD_ID]: {
      metadata: mockModMetadataNuked,
      config: mockModConfigNuked,
      latestVersion: null,
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
  installedModSourceData: (modId: string) => {
    if (modId === LARGE_MOD_ID) {
      return {
        source: largeModSourceInstalled,
        metadata: mockModMetadataLarge,
        readme: mockReadme,
        initialSettings: mockInitialSettings,
      };
    }
    if (modId === ANNOTATED_MOD_ID) {
      return {
        source: '// Mock local source...\n',
        metadata: mockModMetadataAnnotated,
        readme: mockReadme,
        initialSettings: mockAnnotatedInitialSettings,
      };
    }
    return {
      source: '// Mock local source...\n',
      metadata: modId === NUKED_MOD_ID ? mockModMetadataNuked : mockModMetadata,
      readme: mockReadme,
      initialSettings: mockInitialSettings,
    };
  },

  modSettings: (modId: string) =>
    modId === ANNOTATED_MOD_ID
      ? mockAnnotatedModSettings
      : {
          'mock-setting': 'mock-setting-value',
          'mock-setting-dropdown': 'mock-setting-value',
          'mock-setting-array[0]': 'a',
          'mock-setting-array[1]': 'b',
          'mock-setting-array[2]': 'c',
        },

  savedModSettings: {},

  // Keyed by the paths of mockAnnotatedInitialSettings. The runtime entry for
  // `monitor` that names the declared option's value is what the merge
  // relabels it by.
  modDynamicSelectOptions: {
    outputDevice: [
      {
        value: '{0.0.0.00000000}.{a1b2c3d4-0000-0000-0000-000000000001}',
        label: 'Speakers (Realtek High Definition Audio)',
      },
      {
        value: '{0.0.0.00000000}.{a1b2c3d4-0000-0000-0000-000000000002}',
        label: 'Headphones',
      },
    ],
    monitor: [
      { value: 'primary', label: 'Primary monitor (runtime)' },
      { value: '\\\\.\\DISPLAY2', label: 'DISPLAY2 - Dell U2720Q' },
    ],
    'rules.device': [{ value: 'usb::0483:5740', label: 'USB Serial Device (COM3)' }],
    watchedDevices: [
      { value: 'usb::046d:c52b', label: 'USB Receiver (Logitech)' },
      { value: 'usb::0483:5740', label: 'USB Serial Device (COM3)' },
    ],
  },

  pickedFilePath: 'C:\\Users\\me\\Music\\chime.wav',
  pickedFolderPath: 'C:\\Users\\me\\Documents\\Logs',

  fontFamilies: ['Arial', 'Cascadia Code', 'Consolas', 'Segoe UI', 'Segoe UI Variable'],

  // Ctrl+Alt+F5, with Ctrl seen first and Alt after it.
  hotkeyCapture: {
    heldOnTheWay: [
      { ctrl: true, alt: false, shift: false, win: false },
      { ctrl: true, alt: true, shift: false, win: false },
    ],
    hotkey: 'ctrl+alt+116',
    delayMs: 600,
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
    [ANNOTATED_MOD_ID]: mockModConfig,
    [NUKED_MOD_ID]: mockModConfigNuked,
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
  // Mod reviews
  // ============================================================================

  modReviews: (modId: string) => mockModReviews[modId] ?? null,

  postModReview: () => Promise.resolve({ id: nextMockReviewId++ }),

  // A vote on the review the server counts at zero, cast long enough ago for
  // the server to be assumed to have it: what the modal draws for it is the
  // floor a voted review never reads below.
  reviewVotes: {
    'custom-message-box': [{ reviewId: 102, timestamp: 1757000000 }],
  },

  commentsOverNetwork: false,

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
