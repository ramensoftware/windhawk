/**
 * Per-mod persistence of hand-formatted settings YAML.
 *
 * The settings editor regenerates YAML from the flat settings object, which
 * drops convenience data the object can't hold (comments, blank-line
 * separation). This module keeps the exact text a user saved so it can be
 * restored later - as long as it still represents the same settings, which the
 * caller verifies before reuse.
 *
 * Backing store is best-effort localStorage: a single JSON map keyed by mod id,
 * bounded to the most recently saved mods. Losing an entry is harmless; the
 * editor falls back to regenerating YAML from the settings.
 */

const STORAGE_KEY = 'windhawk-modSettingsYaml';

// Cap the number of mods whose formatted YAML is retained, evicting the
// least-recently saved first. The text is KB-scale, so this stays well under the
// localStorage quota while keeping the total bounded and self-pruning.
const MAX_STORED_MODS = 500;

type YamlByModId = Record<string, string>;

function readMap(): YamlByModId {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return {};
    }

    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
      return {};
    }

    // Keep only string entries so a malformed map degrades to empty rather than
    // surfacing junk to callers.
    const map: YamlByModId = {};
    for (const [modId, value] of Object.entries(parsed)) {
      if (typeof value === 'string') {
        map[modId] = value;
      }
    }

    return map;
  } catch {
    return {};
  }
}

export function readSavedYaml(modId: string): string | null {
  const saved = readMap()[modId];
  return typeof saved === 'string' ? saved : null;
}

export function saveYaml(modId: string, yaml: string): void {
  try {
    const map = readMap();

    // Re-insert so this mod becomes the most-recently saved entry; object key
    // order is insertion order, which is what the eviction below relies on.
    delete map[modId];
    map[modId] = yaml;

    const modIds = Object.keys(map);
    for (const staleModId of modIds.slice(0, Math.max(0, modIds.length - MAX_STORED_MODS))) {
      delete map[staleModId];
    }

    localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
  } catch {
    // Ignore storage failures (e.g. restricted webview storage or exceeded
    // quota); the editor falls back to regenerating YAML from the settings.
  }
}

// Exported for testing only.
export const exportedForTesting = {
  STORAGE_KEY,
  MAX_STORED_MODS,
};
