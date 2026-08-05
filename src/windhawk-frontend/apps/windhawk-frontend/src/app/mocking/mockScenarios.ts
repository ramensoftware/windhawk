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
