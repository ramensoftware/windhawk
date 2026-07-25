// The selection model shared by the Export and Import dialogs. The webview IPC
// contract's UserDataSelection is a scope keyword plus per-mod facet overrides; this
// module maps the dialogs' per-row checkbox state onto that shape (pure, so it is
// unit-tested on its own). Export and import speak the identical selection; they
// differ only in where the rows come from (installed mods vs the archive manifest)
// and which facets a row can offer.

import { getDisplayModId, isLocalModId } from '@app/utils';
import type {
  GetInstalledModsReplyData,
  UserDataManifest,
  UserDataModScope,
  UserDataSelection,
} from '@app/webviewIPCMessages';

// One mod row a selection form renders: its identity plus which facets are available
// to toggle. Export always offers both; import offers only the facets the archive
// carries (from the manifest), so an absent facet renders disabled.
export type UserDataModRow = {
  modId: string;
  // Display label: the mod's name, or its (prefix-stripped) id when unnamed.
  name: string;
  isLocal: boolean;
  canSettings: boolean;
  canConfig: boolean;
};

// The per-mod checkbox state a form tracks: whether the mod is included, and its two
// facet toggles. A facet the row cannot offer is held false and never emitted.
export type UserDataModRowState = {
  included: boolean;
  settings: boolean;
  config: boolean;
};

// The whole form's state: the app-settings toggle plus the per-mod map, keyed by
// modId.
export type UserDataSelectionState = {
  appSettings: boolean;
  perMod: Record<string, UserDataModRowState>;
};

// Order rows non-local first, then by display name (case-insensitive) - the same
// ordering the local mods browser uses, so the dialogs read consistently.
function sortRows(rows: UserDataModRow[]): UserDataModRow[] {
  return [...rows].sort((a, b) => {
    if (a.isLocal !== b.isLocal) {
      return a.isLocal ? 1 : -1;
    }
    return a.name.toLowerCase().localeCompare(b.name.toLowerCase());
  });
}

// Export rows: every installed mod, both facets available (export can always read a
// mod's settings and config).
export function exportRowsFromInstalledMods(
  installedMods: GetInstalledModsReplyData['installedMods']
): UserDataModRow[] {
  const rows = Object.entries(installedMods).map(([modId, mod]) => ({
    modId,
    name: mod.metadata?.name || getDisplayModId(modId),
    isLocal: isLocalModId(modId),
    canSettings: true,
    canConfig: true,
  }));
  return sortRows(rows);
}

// Import rows: one per mod the manifest lists, each offering only the facets the
// archive actually carries.
export function importRowsFromManifest(
  manifest: UserDataManifest
): UserDataModRow[] {
  const rows = manifest.mods.map((mod) => ({
    modId: mod.modId,
    name: mod.name || getDisplayModId(mod.modId),
    isLocal: mod.isLocal,
    canSettings: mod.hasSettings,
    canConfig: mod.hasConfig,
  }));
  return sortRows(rows);
}

// The default export state (OQ10): everything on - all mods, settings and config, and
// app settings - the backup case.
export function initialExportState(
  rows: UserDataModRow[]
): UserDataSelectionState {
  const perMod: Record<string, UserDataModRowState> = {};
  for (const row of rows) {
    perMod[row.modId] = { included: true, settings: true, config: true };
  }
  return { appSettings: true, perMod };
}

// The default import state: everything the archive carries pre-checked, with each
// facet on only where the archive holds it (so the "not in this archive" facets stay
// off and disabled).
export function initialImportState(
  manifest: UserDataManifest,
  rows: UserDataModRow[]
): UserDataSelectionState {
  const perMod: Record<string, UserDataModRowState> = {};
  for (const row of rows) {
    perMod[row.modId] = {
      included: true,
      settings: row.canSettings,
      config: row.canConfig,
    };
  }
  return { appSettings: manifest.hasAppSettings, perMod };
}

// Collapse the per-row include flags into the contract's scope keyword when they land
// on a recognizable set, else an explicit id list. Purely cosmetic - an explicit
// { ids } list of exactly the same mods is equivalent - but it keeps the emitted
// request legible and matches the CLI's vocabulary.
export function deriveModScope(
  rows: UserDataModRow[],
  state: UserDataSelectionState
): UserDataModScope {
  const includedIds = rows
    .filter((row) => state.perMod[row.modId]?.included)
    .map((row) => row.modId);

  if (includedIds.length === 0) {
    return 'none';
  }

  const included = new Set(includedIds);
  if (rows.every((row) => included.has(row.modId))) {
    return 'all';
  }

  const localRows = rows.filter((row) => row.isLocal);
  const nonLocalRows = rows.filter((row) => !row.isLocal);
  const isAllExceptLocal =
    localRows.length > 0 &&
    nonLocalRows.length > 0 &&
    localRows.every((row) => !included.has(row.modId)) &&
    nonLocalRows.every((row) => included.has(row.modId));
  if (isAllExceptLocal) {
    return 'all-except-local';
  }

  return { ids: includedIds };
}

// Project the form state onto the wire selection. Every included row gets an explicit
// perMod entry (its facets masked by what the row can offer), so `defaults` never has
// to be relied on and the applied facets are exactly what the checkboxes show.
export function buildSelection(
  rows: UserDataModRow[],
  state: UserDataSelectionState
): UserDataSelection {
  const perMod: UserDataSelection['perMod'] = {};
  for (const row of rows) {
    const rowState = state.perMod[row.modId];
    if (!rowState || !rowState.included) {
      continue;
    }
    perMod[row.modId] = {
      settings: row.canSettings && rowState.settings,
      config: row.canConfig && rowState.config,
    };
  }

  return {
    appSettings: state.appSettings,
    mods: deriveModScope(rows, state),
    defaults: { settings: true, config: true },
    perMod,
  };
}

// Whether the selection would actually do anything: at least app settings or one mod.
// Both dialogs disable their confirm button when nothing is selected.
export function isSelectionEmpty(
  rows: UserDataModRow[],
  state: UserDataSelectionState
): boolean {
  if (state.appSettings) {
    return false;
  }
  return !rows.some((row) => state.perMod[row.modId]?.included);
}
