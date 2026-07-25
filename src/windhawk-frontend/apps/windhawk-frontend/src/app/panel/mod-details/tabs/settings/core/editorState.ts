/**
 * Pure state model for the mod settings editor.
 *
 * The editor works over a flat settings map (`ModSettings`, dotted/bracketed
 * keys) in one of two modes: a form ("ui") or a YAML text buffer. This module
 * owns the single source of truth for that editing state as a reducer, plus the
 * pure helpers that derive dirtiness and rewrite array-index keys.
 *
 * It holds no React, i18n, storage, YAML, or IPC. Anything that needs those
 * (parsing YAML, reading saved text, talking to the backend) is done by the
 * hook, which then dispatches the plain actions below. `resolveInitialYaml`
 * takes those side-effecting pieces as injected dependencies so it stays pure
 * and testable.
 */

import { type ModSettings } from './yamlConverter';

// ============================================================================
// State and actions
// ============================================================================

export type UiWorking = {
  mode: 'ui';
  draft: ModSettings;
  // Prefixes for which the user added an array row that has no materialized
  // keys yet (an empty row). Maps a prefix to its highest requested index.
  arrayMaxIndex: Record<string, number>;
};

export type YamlWorking = {
  mode: 'yaml';
  text: string;
  edited: boolean;
  // The ui draft captured when entering YAML mode, restored verbatim if the
  // user leaves without editing (regenerating from text would drop keys that
  // toYaml trims, e.g. values cleared to '' or 0).
  sourceDraft: ModSettings;
};

export type Working = UiWorking | YamlWorking;

export type EditorState =
  | { status: 'loading' }
  | { status: 'ready'; saved: ModSettings; working: Working };

export type EditorAction =
  | { type: 'loaded'; saved: ModSettings; working: Working }
  | { type: 'changeSetting'; key: string; value: string | number }
  | { type: 'addArrayItem'; prefix: string; index: number }
  | { type: 'removeArrayItem'; prefix: string; index: number }
  | { type: 'enterYamlMode'; text: string }
  | { type: 'setYamlText'; text: string }
  | { type: 'exitYamlMode'; draft: ModSettings }
  | { type: 'saveSucceeded'; savedSettings: ModSettings; savedText?: string };

export const initialEditorState: EditorState = { status: 'loading' };

export function makeUiWorking(draft: ModSettings): UiWorking {
  return { mode: 'ui', draft, arrayMaxIndex: {} };
}

export function makeYamlWorking(text: string, sourceDraft: ModSettings): YamlWorking {
  return { mode: 'yaml', text, edited: false, sourceDraft };
}

// ============================================================================
// Array-key helpers
// ============================================================================

/**
 * The highest array index materialized under `prefix` by the keys present in
 * `settings` (e.g. `foo[2].bar` contributes index 2 for prefix `foo`).
 * Returns -1 when no such key exists.
 */
export function materializedMaxIndex(settings: ModSettings, prefix: string): number {
  const open = prefix + '[';
  return Object.keys(settings).reduce((maxIndex, key) => {
    if (key.startsWith(open)) {
      const match = key.slice(open.length).match(/^(\d+)\]/);
      if (match) {
        return Math.max(maxIndex, parseInt(match[1], 10));
      }
    }
    return maxIndex;
  }, -1);
}

/**
 * The array index a key occupies directly under `prefix` (e.g. `foo[1].bar`
 * under `foo` is index 1), or null if the key is not an element of that array.
 */
export function indexAtPrefix(key: string, prefix: string): number | null {
  const open = prefix + '[';
  if (key.startsWith(open)) {
    const match = key.slice(open.length).match(/^(\d+)\]/);
    if (match) {
      return parseInt(match[1], 10);
    }
  }
  return null;
}

/**
 * Decrements the array index of a key under `prefix` when it sits after a
 * removed `index`, so surviving elements close the gap (`foo[2]` -> `foo[1]`
 * when index 1 is removed). Keys outside the array, or at/before the removed
 * index, are returned unchanged.
 */
export function rewriteKeyAfterRemove(key: string, prefix: string, index: number): string {
  const open = prefix + '[';
  if (key.startsWith(open)) {
    const match = key.slice(open.length).match(/^(\d+)(\].*$)/);
    if (match) {
      const keyIndex = parseInt(match[1], 10);
      if (keyIndex > index) {
        return prefix + '[' + (keyIndex - 1).toString() + match[2];
      }
    }
  }
  return key;
}

function applyRemoveArrayItem(working: UiWorking, prefix: string, index: number): UiWorking {
  const draft = Object.fromEntries(
    Object.entries(working.draft)
      .filter(([key]) => indexAtPrefix(key, prefix) !== index)
      .map(([key, value]) => [rewriteKeyAfterRemove(key, prefix, index), value])
  );

  const arrayMaxIndex = Object.fromEntries(
    Object.entries(working.arrayMaxIndex)
      .filter(([key]) => indexAtPrefix(key, prefix) !== index)
      .map(([key, value]) =>
        key === prefix
          ? [key, Math.max(value - 1, 0)]
          : [rewriteKeyAfterRemove(key, prefix, index), value]
      )
  );

  return { mode: 'ui', draft, arrayMaxIndex };
}

// ============================================================================
// Dirtiness
// ============================================================================

/**
 * Structural equality of two flat settings maps: same keys, same scalar value
 * per key. Values are `string | number` with schema-stable types, so `===` is
 * exact (a number 0 and a string '0' are correctly unequal).
 */
export function settingsEqual(a: ModSettings, b: ModSettings): boolean {
  const aKeys = Object.keys(a);
  if (aKeys.length !== Object.keys(b).length) {
    return false;
  }
  return aKeys.every(
    (key) => Object.prototype.hasOwnProperty.call(b, key) && a[key] === b[key]
  );
}

export function isDirty(state: EditorState): boolean {
  if (state.status !== 'ready') {
    return false;
  }

  const { working, saved } = state;
  if (working.mode === 'yaml') {
    // Dirty if the buffer was hand-edited, or if it was seeded from a draft
    // that already differed from the saved baseline (e.g. switching to YAML
    // with unsaved form changes). The unedited buffer round-trips to
    // sourceDraft, so comparing that draft to saved is the content check.
    return working.edited || !settingsEqual(working.sourceDraft, saved);
  }

  // An added-but-empty array row (tracked in arrayMaxIndex for rendering)
  // materializes no keys and persists nothing, so it is not a change on its
  // own; dirtiness comes only from values that differ from the saved baseline.
  return !settingsEqual(working.draft, saved);
}

// ============================================================================
// Reducer
// ============================================================================

export function editorReducer(state: EditorState, action: EditorAction): EditorState {
  if (action.type === 'loaded') {
    return { status: 'ready', saved: action.saved, working: action.working };
  }

  if (state.status !== 'ready') {
    return state;
  }

  const { working } = state;

  switch (action.type) {
    case 'changeSetting':
      if (working.mode !== 'ui') {
        return state;
      }
      return {
        ...state,
        working: {
          ...working,
          draft: { ...working.draft, [action.key]: action.value },
        },
      };

    case 'addArrayItem':
      if (working.mode !== 'ui') {
        return state;
      }
      return {
        ...state,
        working: {
          ...working,
          arrayMaxIndex: { ...working.arrayMaxIndex, [action.prefix]: action.index },
        },
      };

    case 'removeArrayItem':
      if (working.mode !== 'ui') {
        return state;
      }
      return { ...state, working: applyRemoveArrayItem(working, action.prefix, action.index) };

    case 'enterYamlMode':
      if (working.mode !== 'ui') {
        return state;
      }
      return {
        ...state,
        working: makeYamlWorking(action.text, working.draft),
      };

    case 'setYamlText':
      if (working.mode !== 'yaml') {
        return state;
      }
      return { ...state, working: { ...working, text: action.text, edited: true } };

    case 'exitYamlMode':
      if (working.mode !== 'yaml') {
        return state;
      }
      return { ...state, working: makeUiWorking(action.draft) };

    case 'saveSucceeded': {
      const keepsYamlEdited =
        working.mode === 'yaml' &&
        action.savedText !== undefined &&
        working.text === action.savedText;
      return {
        status: 'ready',
        saved: action.savedSettings,
        working: keepsYamlEdited
          ? { ...(working as YamlWorking), edited: false, sourceDraft: action.savedSettings }
          : working,
      };
    }
  }
}

// ============================================================================
// Initial YAML resolution (pure, dependency-injected)
// ============================================================================

export type ResolveInitialYamlDeps = {
  readSavedYaml: (modId: string) => string | null;
  settingsToYaml: (settings: ModSettings) => string;
  yamlToSettings: (yamlString: string) => { settings: ModSettings | null; error: string | null };
};

/**
 * Picks the YAML text to show when entering YAML mode. Prefers the user's saved
 * hand-formatted YAML (comments, blank lines) when it still round-trips to the
 * same settings being loaded; otherwise regenerates from the settings. Both
 * sides are normalized through settingsToYaml so formatting differences alone
 * don't force a regeneration.
 */
export function resolveInitialYaml(
  modId: string,
  settings: ModSettings,
  deps: ResolveInitialYamlDeps
): string {
  const saved = deps.readSavedYaml(modId);
  // Ignore empty or whitespace-only saved text: it carries no formatting worth
  // restoring, and reusing it would show a blank editor in place of freshly
  // generated YAML.
  if (saved !== null && saved.trim() !== '') {
    const { settings: parsed, error } = deps.yamlToSettings(saved);
    if (!error && parsed && deps.settingsToYaml(parsed) === deps.settingsToYaml(settings)) {
      return saved;
    }
  }
  return deps.settingsToYaml(settings);
}

// Exported for testing only.
export const exportedForTesting = {
  settingsEqual,
  indexAtPrefix,
  rewriteKeyAfterRemove,
};
