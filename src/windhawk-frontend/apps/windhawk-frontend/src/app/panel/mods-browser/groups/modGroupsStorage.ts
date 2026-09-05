/**
 * Where the mod groups live between visits: one JSON blob in localStorage.
 *
 * Best-effort, in the shape yamlStorage already established. Storage can be
 * closed to a webview entirely, and reading it there throws rather than coming
 * back empty - so both sides swallow, and the feature works for as long as the
 * screen is open. Nothing bounds the size: a group is a name and a list of mod
 * ids, so even a heavily grouped machine stores a few kilobytes.
 */

import { readStoredValue, writeStoredValue } from '@app/utils';
import { type ModGroup, parseModGroups, serializeModGroups } from './modGroups';

const STORAGE_KEY = 'windhawk-modGroups';

export function readModGroups(): ModGroup[] {
  const raw = readStoredValue(STORAGE_KEY);
  if (!raw) {
    return [];
  }

  try {
    return parseModGroups(JSON.parse(raw));
  } catch {
    // Text that does not parse comes to the same thing as nothing stored: a
    // screen with no groups, which is what a fresh install is.
    return [];
  }
}

export function writeModGroups(groups: ModGroup[]): void {
  writeStoredValue(STORAGE_KEY, JSON.stringify(serializeModGroups(groups)));
}

// Exported for testing only.
export const exportedForTesting = {
  STORAGE_KEY,
};
