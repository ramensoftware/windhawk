/**
 * Named variants of mock mode, for the states `defaultMockData` cannot reach.
 *
 * The default fixtures describe one healthy machine where every command succeeds,
 * which leaves the screens that only appear when something is missing or goes
 * wrong - an empty install list, "loading failed", a command's error notification -
 * with no way to be seen in the browser preview or driven by a test. A scenario is
 * that same mock host with a few answers replaced.
 *
 * Pick one with `?mock=<name>` in the URL, or by setting `windhawk-mock-scenario`
 * in localStorage and reloading. Neither is read in a real webview: mock mode is
 * only on when there is no host to answer.
 *
 * A scenario replaces two kinds of answer:
 *
 * - `data` swaps registry slices, so a screen renders over different fixtures.
 * - `replies` rewrites a command's reply after its selector produced one, which
 *   is how a failure is injected. The reply goes through the same dispatch a
 *   host's does, so an error it carries surfaces exactly as a real one would.
 */

import { ReviewPostError } from '@app/panel/shared/reviews/reviewPostError';
import { ALL_VERSIONS, formatSuppression } from '@app/webviewIPCMessages';
import type { MockDataRegistry } from './MockRegistry';
import {
  defaultMockData,
  mockAnnotatedInitialSettings,
  mockAnnotatedModSettings,
  mockLongReadme,
  mockNukedReason,
} from './MockRegistry';

const SCENARIO_PARAM = 'mock';
const SCENARIO_STORAGE_KEY = 'windhawk-mock-scenario';

type WireReply = Record<string, unknown>;

export type MockScenario = {
  // What this scenario stands for, shown when an unknown name is asked for.
  description: string;
  data?: Partial<MockDataRegistry>;
  replies?: Record<string, (reply: WireReply) => WireReply>;
};

// A command that ran and failed, in the shape a reply carries it: the outcome
// flag the handler reads, plus the error object the IPC layer surfaces.
function failsWith(code: string, message: string) {
  return (reply: WireReply): WireReply => ({
    ...reply,
    succeeded: false,
    error: { code, message },
  });
}

type InstalledMod = MockDataRegistry['installedMods'][string];

// An installed mod off the default one, named and in a state of its own. The
// default machine has every mod compiled and switched on, so the states the
// enable/disable rules are about have to be built rather than picked.
function installedModNamed(
  name: string,
  config: InstalledMod['config']
): InstalledMod {
  const mod = defaultMockData.installedMods['custom-message-box'];
  return {
    ...mod,
    metadata: { ...mod.metadata, name },
    config,
    // Nothing waiting names no version to have it waiting at; a mod here that
    // wants an offer names one.
    latestVersion: null,
  };
}

const enabledConfig = defaultMockData.modConfig['custom-message-box'];

// The mod the default fixtures put on the machine and in the repository listing
// both, which is the one a scenario can describe to both browsers at once.
const SAMPLE_MOD_ID = 'custom-message-box';

// The version the repository offers for a mod, which is what a pin has to name
// to suppress anything - read from the fixture that answers for it rather than
// spelled again here, so a pin cannot come to name a version no longer offered.
function repositoryVersionOf(modId: string): string {
  return defaultMockData.modVersionSource(modId).metadata?.version ?? '';
}

type RepositoryMod = MockDataRegistry['repositoryMods'][string];

// A mod's entry in the repository listing, named and at the version the
// repository fixture hands out for it, flagged by the catalog for `nuked` where
// one is given.
function catalogEntryNamed(
  modId: string,
  name: string,
  nuked?: string
): RepositoryMod {
  const { details } = defaultMockData.repositoryMods['online001'].repository;
  return {
    repository: {
      metadata: { ...defaultMockData.modVersionSource(modId).metadata, name },
      details: nuked === undefined ? details : { ...details, nuked },
    },
  };
}

const nukedConfig = { ...enabledConfig, nuked: mockNukedReason };

export const mockScenarios: Record<string, MockScenario> = {
  empty: {
    description: 'A fresh machine: nothing installed, nothing to feature.',
    data: {
      installedMods: {},
      featuredMods: {},
      repositoryMods: {},
    },
  },

  offline: {
    description: 'The mod repository cannot be reached.',
    replies: {
      // Both lists report their absence as a null payload, which is what the
      // browsers render their "loading failed" message from.
      getFeaturedMods: () => ({ featuredMods: null }),
      getRepositoryMods: () => ({ mods: null }),
    },
  },

  'mixed-mod-states': {
    description:
      'Installed mods in the states a healthy machine has none of: one enabled, one disabled, one never compiled.',
    data: {
      // What decides which mods a batch action reaches: a mod already in the
      // state asked for needs no request, and one that was never compiled can be
      // neither enabled nor disabled - it is skipped rather than blocking the
      // action. The local mod is here because a selection holds one like any
      // other. Every mod in the default slice is compiled and running, which
      // exercises none of that, and the default stands for a healthy machine.
      installedMods: {
        'enabled-mod': installedModNamed('Enabled mod', enabledConfig),
        'disabled-mod': installedModNamed('Disabled mod', {
          ...enabledConfig,
          disabled: true,
        }),
        'never-compiled-mod': installedModNamed('Never compiled mod', null),
        'local@edited-mod': installedModNamed('Edited mod', enabledConfig),
      },
    },
  },

  'user-scoped': {
    description:
      'Mods with a configuration of the user\'s own: one in use, one kept but switched off.',
    data: {
      // The listing and getModConfig answer from two slices, so both name the
      // scope. The default machine has no user configuration anywhere, which
      // leaves the icons nothing to be drawn for.
      installedMods: {
        'own-config-mod': installedModNamed('Own config mod', {
          ...enabledConfig,
          userScope: 'active',
        }),
        'own-config-inactive-mod': installedModNamed(
          'Own config inactive mod',
          { ...enabledConfig, userScope: 'inactive' }
        ),
      },
      modConfig: {
        'own-config-mod': { ...enabledConfig, userScope: 'active' },
        'own-config-inactive-mod': { ...enabledConfig, userScope: 'inactive' },
      },
    },
  },

  nuked: {
    description:
      'Mods the Windhawk mod catalog took off the machine: one waiting on the catalog, one an update restores, one taken off since it was listed, and a flagged mod that is not installed.',
    data: {
      // Each nuked mod was running when the catalog took it off, so its switch
      // still says so - which is what has to be drawn off regardless. The
      // listing and getModConfig answer from two slices: the mod taken off since
      // it was listed is listed as the user left it, off and not nuked, and is
      // nuked in the config the host reads and refuses to enable it over.
      installedMods: {
        // The catalog still flags the mod, so the host caches no version to
        // offer.
        'nuked-mod': installedModNamed('Nuked mod', nukedConfig),
        // The catalog has moved on to a clean version, which is offered, and
        // installing it restores the mod.
        'nuked-updatable-mod': {
          ...installedModNamed('Nuked updatable mod', nukedConfig),
          latestVersion: repositoryVersionOf('nuked-updatable-mod'),
        },
        'newly-nuked-mod': installedModNamed('Newly nuked mod', {
          ...enabledConfig,
          disabled: true,
        }),
        // One a batch enable has something to do for.
        'disabled-mod': installedModNamed('Disabled mod', {
          ...enabledConfig,
          disabled: true,
        }),
      },
      modConfig: {
        'nuked-mod': nukedConfig,
        'nuked-updatable-mod': nukedConfig,
        'newly-nuked-mod': { ...nukedConfig, disabled: true },
        'disabled-mod': { ...enabledConfig, disabled: true },
      },
      repositoryMods: {
        ...defaultMockData.repositoryMods,
        'nuked-mod': catalogEntryNamed(
          'nuked-mod',
          'Nuked mod',
          mockNukedReason
        ),
        'nuked-updatable-mod': catalogEntryNamed(
          'nuked-updatable-mod',
          'Nuked updatable mod'
        ),
        'flagged-mod': catalogEntryNamed(
          'flagged-mod',
          'Flagged mod',
          mockNukedReason
        ),
      },
      featuredMods: {
        'flagged-mod': catalogEntryNamed(
          'flagged-mod',
          'Flagged mod',
          mockNukedReason
        ).repository,
      },
    },
  },

  'updates-disabled': {
    description:
      'Mods whose update offers the user turned off: one pinned to a version, one for good, and the sample mod both browsers list.',
    data: {
      // What a mod looks like once its offer is suppressed: the host stops
      // reporting an update for it while still naming the version it would have
      // been for, which is what tells this from a mod that is up to date. That
      // is the state the allow-updates button is for, and no default fixture
      // reaches it - every mod there either has an offer or nothing to
      // suppress.
      installedMods: {
        'pinned-mod': {
          ...installedModNamed('Pinned mod', {
            ...enabledConfig,
            updatesDisabledForVersion: formatSuppression({
              kind: 'pinned',
              version: repositoryVersionOf('pinned-mod'),
            }),
          }),
          latestVersion: repositoryVersionOf('pinned-mod'),
        },
        'never-update-mod': {
          ...installedModNamed('Never update mod', {
            ...enabledConfig,
            updatesDisabledForVersion: ALL_VERSIONS,
          }),
          latestVersion: repositoryVersionOf('never-update-mod'),
        },
        'updatable-mod': {
          ...installedModNamed('Updatable mod', enabledConfig),
          latestVersion: repositoryVersionOf('updatable-mod'),
        },
        // The sample mod, refused the version on offer. It is the one mod the
        // repository listing also holds, so it is the state both browsers
        // answer about, and both reach that answer the same way - off this
        // config and the version beside it.
        [SAMPLE_MOD_ID]: {
          ...defaultMockData.installedMods[SAMPLE_MOD_ID],
          config: {
            ...enabledConfig,
            updatesDisabledForVersion: formatSuppression({
              kind: 'pinned',
              version: repositoryVersionOf(SAMPLE_MOD_ID),
            }),
          },
        },
      },
    },
  },

  'translated-catalog': {
    description:
      'The repository listing as a language catalog serves it: one mod translated, named again in the language it was published in.',
    data: {
      // What a catalog under `catalogs/<language>.json` carries for a mod
      // someone has translated: `metadata` in that language, `metadataEnglish`
      // as published. The default fixtures are the English catalog, where a mod
      // has the one name. Only the catalog is in another language here - the app
      // stays in English, since what this is for is the shape of an entry rather
      // than the app's own translation.
      repositoryMods: {
        ...defaultMockData.repositoryMods,
        'taskbar-customization': {
          repository: {
            metadata: {
              name: 'Taskleiste anpassen',
              description: 'Passt die Taskleiste an',
              version: '1.0',
              author: 'John Smith',
            },
            metadataEnglish: {
              name: 'Customize the taskbar',
              description: 'Customizes the taskbar',
              version: '1.0',
              author: 'John Smith',
            },
            details: defaultMockData.repositoryMods['online001'].repository.details,
          },
        },
      },
    },
  },

  'unreadable-mod-source': {
    description:
      "An installed mod whose source the host cannot read: it is on the machine, and nothing of it can be shown.",
    data: {
      // The state the host builds a mod entry in when its config is there and its
      // source file is not: the listing is the union of the two, so the mod is
      // reported with a config and no metadata at all. Its installed version
      // reads as empty, which differs from whatever the repository last offered,
      // so the host reports an update for it as well - this is the mod with an
      // offer standing over a copy that cannot be read.
      installedMods: {
        // No metadata means no name either, so the lists have only its id to
        // call it by - which is what a real one of these looks like.
        'unreadable-mod': {
          metadata: null,
          config: enabledConfig,
          latestVersion: repositoryVersionOf('unreadable-mod'),
          userRating: 0,
        },
        'readable-mod': installedModNamed('Readable mod', enabledConfig),
      },
    },
    replies: {
      // What the host answers a source request for that mod with: a reply whose
      // every field is absent, rather than no reply. The screen tells the read
      // that failed from the one still on its way by which of those it has.
      getModSourceData: (reply) =>
        reply['modId'] === 'unreadable-mod'
          ? {
              ...reply,
              data: {
                source: null,
                metadata: null,
                readme: null,
                initialSettings: null,
              },
            }
          : reply,
    },
  },

  'annotated-settings': {
    description:
      "Every mod's settings block is the annotated sample mod's, on the repository preview as well as the settings tab.",
    data: {
      // The one block on both sides - the installed mod's settings tab and the
      // repository preview of it - so the controls and their read-only
      // renderings are drawn from the same declarations. The default machine
      // has the block on one installed mod only, which puts it in front of no
      // preview.
      installedModSourceData: (modId: string) => ({
        ...defaultMockData.installedModSourceData(modId),
        initialSettings: mockAnnotatedInitialSettings,
      }),
      modVersionSource: (modId: string, version?: string) => ({
        ...defaultMockData.modVersionSource(modId, version),
        initialSettings: mockAnnotatedInitialSettings,
      }),
      modSettings: () => mockAnnotatedModSettings,
    },
  },

  'long-readme': {
    description:
      "Every mod's readme runs well past the viewport, its link to another mod at the foot.",
    // Both sides of every mod, so the page scrolls whichever version it opens
    // on and whichever mod the link leads to.
    data: {
      installedModSourceData: (modId: string) => ({
        ...defaultMockData.installedModSourceData(modId),
        readme: mockLongReadme,
      }),
      modVersionSource: (modId: string, version?: string) => ({
        ...defaultMockData.modVersionSource(modId, version),
        readme: mockLongReadme,
      }),
    },
  },

  'hotkey-capture-unavailable': {
    description:
      "The annotated sample mod on a host that cannot record shortcuts, so a hotkey's badge falls back to its manual editor.",
    // The same block as annotated-settings: the badge under test is that mod's.
    data: {
      installedModSourceData: (modId: string) => ({
        ...defaultMockData.installedModSourceData(modId),
        initialSettings: mockAnnotatedInitialSettings,
      }),
      modSettings: () => mockAnnotatedModSettings,
    },
    replies: {
      // What a host answers a capture with when it cannot start one: no
      // shortcut and no reason it ended, but the error - an older core not
      // knowing the command, or a keyboard hook that could not be installed.
      captureHotkey: () => ({
        hotkey: null,
        error: {
          code: 'HOTKEY_CAPTURE_UNAVAILABLE',
          message: 'hotkey capture: SetWindowsHookExW failed: Access is denied.',
        },
      }),
    },
  },

  'command-failure': {
    description: 'Every mod command the host is asked to run fails.',
    replies: {
      enableMod: failsWith('ENABLE_FAILED', 'The mod could not be enabled.'),
      deleteMod: failsWith('DELETE_FAILED', 'The mod could not be removed.'),
      setModSettings: failsWith(
        'SETTINGS_FAILED',
        'The settings could not be saved.'
      ),
      updateModRating: failsWith('RATING_FAILED', 'The rating was not recorded.'),
      // A failed switch or delete carries no config.
      setModUserScope: (reply) =>
        failsWith(
          'USER_SCOPE_FAILED',
          'The user configuration could not be switched.'
        )({ modId: reply['modId'] }),
      deleteModUserConfig: (reply) =>
        failsWith(
          'USER_CONFIG_DELETE_FAILED',
          'The user configuration could not be deleted.'
        )({ modId: reply['modId'] }),
      // Install reports its outcome as the details it produced, so a failure is an
      // absent mod rather than succeeded:false.
      installMod: (reply) => ({
        modId: reply['modId'],
        installedModDetails: null,
        error: { code: 'INSTALL_FAILED', message: 'The mod could not be installed.' },
      }),
    },
  },

  'dev-tools-missing': {
    description:
      'The development tools are not on the machine, so a launch offers to install them instead of opening an editor.',
    replies: {
      // What a host answers a launch with when the editor it would open is not
      // there: no error object, because nothing failed - the app raises the
      // install offer off this flag alone.
      createNewMod: () => ({ uiMissing: true }),
      editMod: () => ({ uiMissing: true }),
      forkMod: () => ({ uiMissing: true }),
    },
  },

  'update-source-failure': {
    description: "The repository will not hand over an updatable mod's source.",
    replies: {
      // The source a mod's update needs, reported as absent - which is how an
      // unreachable repository answers, and what leaves a mod that cannot be
      // updated at all.
      getRepositoryModSourceData: (reply) => ({
        ...reply,
        data: {
          source: null,
          metadata: null,
          readme: null,
          initialSettings: null,
        },
      }),
    },
  },

  'update-install-failure': {
    description: 'Every mod update the host is asked to install fails.',
    replies: {
      // A failed install is null details and nothing else: neither host attaches
      // an error object to this reply. What went wrong reaches the user through
      // the compiler output window instead, which is also why COMPILER_FAILED is
      // in AUTO_SURFACE_SKIP. A scenario that invented one here would show a
      // notification that cannot happen.
      installMod: (reply) => ({
        modId: reply['modId'],
        installedModDetails: null,
      }),
    },
  },

  'import-failure': {
    description: 'A user-data import that the host cannot complete.',
    replies: {
      importUserData: failsWith(
        'IMPORT_FAILED',
        'The archive could not be imported.'
      ),
    },
  },

  'review-post-failure': {
    description:
      'The update server turns every review post away: too many from this address.',
    data: {
      // The refusal the server explains, in its words, which the form draws as
      // they are. The status is the rate limit's.
      postModReview: () =>
        Promise.reject(
          new ReviewPostError(429, 'Too many reviews, try again later')
        ),
    },
  },

  'review-votes-failure': {
    description:
      'A host over a core from before the vote commands: all three are unknown to it.',
    replies: {
      // What an older DLL's refusal arrives as: the read's list is empty with
      // the error attached, which the modal takes as no voting at all.
      getModReviewVotes: (reply) => ({
        ...reply,
        votes: [],
        error: {
          code: 'INVALID_REQUEST',
          message: 'unknown command: getModReviewVotes',
        },
      }),
      voteModReview: (reply) => ({
        ...reply,
        votes: [],
        succeeded: false,
        error: {
          code: 'INVALID_REQUEST',
          message: 'unknown command: voteModReview',
        },
      }),
      retractModReviewVote: (reply) => ({
        ...reply,
        votes: [],
        succeeded: false,
        error: {
          code: 'INVALID_REQUEST',
          message: 'unknown command: retractModReviewVote',
        },
      }),
    },
  },

  'review-undo-failure': {
    description:
      'A host over a core that takes a vote but not one back: the retraction alone is unknown to it.',
    replies: {
      // The vote stands as cast: the modal draws it final and says the undo
      // did not go.
      retractModReviewVote: (reply) => ({
        ...reply,
        votes: [],
        succeeded: false,
        error: {
          code: 'INVALID_REQUEST',
          message: 'unknown command: retractModReviewVote',
        },
      }),
    },
  },

  'reviews-unavailable': {
    description: "A mod's reviews document cannot be fetched.",
    data: {
      modReviews: () => {
        throw new Error('The reviews document could not be fetched.');
      },
    },
  },

  'reviews-over-network': {
    description:
      'The reviews document and the post go out over the network instead of being answered here.',
    // So the requests can be inspected as a server would see them: the URL,
    // the method, the headers and the body.
    data: {
      commentsOverNetwork: true,
    },
  },
};

function readScenarioName(): string | null {
  if (typeof window === 'undefined') {
    return null;
  }
  const fromUrl = new URLSearchParams(window.location.search).get(SCENARIO_PARAM);
  if (fromUrl) {
    return fromUrl;
  }
  try {
    return window.localStorage.getItem(SCENARIO_STORAGE_KEY);
  } catch {
    // Storage can be unavailable (a sandboxed frame); no scenario is the answer.
    return null;
  }
}

export function resolveMockScenario(name: string | null): MockScenario | null {
  if (!name) {
    return null;
  }
  const scenario = mockScenarios[name];
  if (!scenario) {
    console.warn(
      `Unknown mock scenario "${name}". Available: ${Object.keys(mockScenarios).join(', ')}`
    );
    return null;
  }
  return scenario;
}

// The scenario this page load runs under, resolved once. A page that swapped
// fixtures mid-flight would not stand for anything a host does, so switching
// scenarios means reloading.
const requestedScenarioName = readScenarioName();
export const activeMockScenario = resolveMockScenario(requestedScenarioName);
// The name of the scenario in force, or null when the defaults are - so a name
// that resolved to nothing reads as no scenario rather than as itself.
export const activeMockScenarioName = activeMockScenario
  ? requestedScenarioName
  : null;

// The registry the app reads, with the active scenario's slices over it. What
// the mock host writes stays in it for the page's lifetime.
export const activeMockData: MockDataRegistry = activeMockScenario?.data
  ? { ...defaultMockData, ...activeMockScenario.data }
  : defaultMockData;

const initialMockData: MockDataRegistry = { ...activeMockData };

// Puts the registry back as the page load found it. A write replaces the slices
// it changes rather than editing them, so this undoes every one.
export function resetMockData() {
  Object.assign(activeMockData, initialMockData);
}

// The reply the mock host answers `command` with, after the active scenario has
// had its say. Returns the selector's own reply when no scenario rewrites it.
export function applyScenarioReply<TReply>(
  command: string,
  reply: TReply
): TReply {
  const override = activeMockScenario?.replies?.[command];
  if (!override) {
    return reply;
  }
  return override(reply as WireReply) as TReply;
}
