// Import-result rows the aborted view renders. A canceled import stops the host loop
// at once, so the mods it never reached emit no wire outcome; the webview reconstructs
// them from the selection and tags them 'aborted', a display-only status with no wire
// counterpart.

import type {
  UserDataImportModOutcome,
  UserDataImportSummary,
} from '@app/webviewIPCMessages';

import type { UserDataSelectionState } from './selection';

// One row in the import result list: a wire outcome status, or 'aborted' for a selected
// mod the canceled import never reached.
export type ImportOutcomeRow = {
  modId: string;
  status: UserDataImportModOutcome['status'] | 'aborted';
  message?: string;
};

// The status of the app-settings row shown above the mods in the progress and result
// lists: the live 'applying'/'applied' the host reports, 'pending' before its first
// marker, or 'aborted' when a canceled/failed import never applied them.
export type AppSettingsOutcomeStatus = 'pending' | 'applying' | 'applied' | 'aborted';

// Order import outcomes by the given mod order, whatever order the host reported them
// in. An outcome whose mod is not in that list (defensive) sorts after the known ones,
// keeping its relative position (the sort is stable).
export function orderOutcomesByModIds<T extends { modId: string }>(
  modIds: string[],
  outcomes: T[]
): T[] {
  const position = new Map(modIds.map((modId, index) => [modId, index]));
  return [...outcomes].sort(
    (a, b) =>
      (position.get(a.modId) ?? modIds.length) -
      (position.get(b.modId) ?? modIds.length)
  );
}

// Every selected mod, in the given mod order, carrying its real outcome or an
// 'aborted' placeholder for a mod the canceled import never reached. The summary holds
// only the mods that finished before the cancel, so all the others are aborted.
export function abortedOutcomeRows(
  modIds: string[],
  state: UserDataSelectionState,
  summary: UserDataImportSummary | null
): ImportOutcomeRow[] {
  const done = new Map(
    (summary?.mods ?? []).map((outcome) => [outcome.modId, outcome] as const)
  );
  return modIds
    .filter((modId) => state.perMod[modId]?.included)
    .map((modId) => done.get(modId) ?? { modId, status: 'aborted' });
}
