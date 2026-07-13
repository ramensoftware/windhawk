// Shared data shapes used by both the VSCode extension layer and the
// upcoming command-line tool. Also the IPC contract the React webview
// side syncs against.
//
// Invariants (lint-enforced):
// - This file is the single source of truth for every shared data type.
// - This file does not import from any other file in the repo.
// - Other files must consume these types from here, not re-declare them.

export type ModConfig = {
  libraryFileName: string;
  disabled: boolean;
  loggingEnabled: boolean;
  debugLoggingEnabled: boolean;
  include: string[];
  exclude: string[];
  includeCustom: string[];
  excludeCustom: string[];
  includeExcludeCustomOnly: boolean;
  patternsMatchCriticalSystemProcesses: boolean;
  architecture: string[];
  version: string;
};

export type AppSettings = {
  language: string;
  disableUpdateCheck: boolean;
  // null in portable mode (the scheduled task only exists in non-portable installs).
  disableRunUIScheduledTask: boolean | null;
  devModeOptOut: boolean;
  hideTrayIcon: boolean;
  alwaysCompileModsLocally: boolean;
  dontAutoShowToolkit: boolean;
  modTasksDialogDelay: number;
  safeMode: boolean;
  loggingVerbosity: number;
  engine: {
    loggingVerbosity: number;
    include: string[];
    exclude: string[];
    injectIntoCriticalProcesses: boolean;
    injectIntoIncompatiblePrograms: boolean;
    injectIntoGames: boolean;
  };
};

export type ModMetadata = Partial<{
  version: string;
  id: string;
  github: string;
  twitter: string;
  homepage: string;
  compilerOptions: string;
  license: string;
  donateUrl: string;
  name: string;
  description: string;
  author: string;
  include: string[];
  exclude: string[];
  architecture: string[];
}>;

export type RepositoryDetails = {
  users: number;
  rating: number;
  // ratingUsers: number;
  ratingBreakdown: number[];
  defaultSorting: number;
  published: number;
  updated: number;
};

export type InitialSettingsValue =
  | boolean
  | number
  | string
  | InitialSettings
  | InitialSettingsArrayValue;

export type InitialSettingsArrayValue = number[] | string[] | InitialSettings[];

export type InitialSettingItem = {
  key: string;
  value: InitialSettingsValue;
  name?: string;
  description?: string;
  options?: Record<string, string>[];
};

export type InitialSettings = InitialSettingItem[];

// Per-mod runtime settings, stored as a flat key/value map.
// Nested/array source declarations are flattened at write time
// (see modSource.extractInitialSettingsForEngine).
export type ModSettings = Record<string, string | number>;

// UI-bootstrap subset surfaced to the webview; derived from AppSettings plus
// update/user-profile state.
export type AppUISettings = {
  language: string;
  devModeOptOut: boolean;
  loggingEnabled: boolean;
  updateIsAvailable: boolean;
  updateIsAvailableBleedingEdge: boolean;
  safeMode: boolean;
};

// One mod's entry in the repository catalog JSON.
export type CatalogEntry = {
  metadata: ModMetadata;
  details: RepositoryDetails;
  featured?: boolean;
};

// The repository catalog (catalogs/<language>.json / catalog.json).
export type Catalog = {
  app: { version?: string; versionBleedingEdge?: string };
  mods: Record<string, CatalogEntry>;
};

// One entry of a mod's versions.json, normalized for consumers (isPreRelease
// is derived from the version string).
export type ModVersionInfo = {
  version: string;
  timestamp: number;
  isPreRelease: boolean;
};
