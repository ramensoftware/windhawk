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

import { ALL_VERSIONS, formatSuppression } from '@app/webviewIPCMessages';
import type { MockDataRegistry } from './MockRegistry';
import { defaultMockData } from './MockRegistry';

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
// default machine has every mod compiled and running, so the states the
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
      // exercises none of that, and flipping one there would contradict the
      // journeys that assert the machine is healthy.
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
      // in AUTO_SURFACE_SKIP. A scenario that invented one here would have the
      // journey assert a notification that cannot happen.
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

// The registry the app reads, with the active scenario's slices over it.
export const activeMockData: MockDataRegistry = activeMockScenario?.data
  ? { ...defaultMockData, ...activeMockScenario.data }
  : defaultMockData;

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
