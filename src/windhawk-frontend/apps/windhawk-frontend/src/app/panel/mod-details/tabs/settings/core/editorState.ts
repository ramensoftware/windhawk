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
  | { type: 'removeAllArrayItems'; prefix: string }
  | { type: 'moveArrayItem'; prefix: string; from: number; to: number }
  | { type: 'resetSetting'; keyPrefix: string; defaults: ModSettings }
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

/**
 * Where an array index lands when the element at `from` is moved to `to`: the
 * moved element takes the target index, and the elements the move steps over
 * shift one place the other way to fill the place it left and open the place it
 * takes. An index outside the range the move spans stays where it is.
 */
function indexAfterMove(index: number, from: number, to: number): number {
  if (index === from) {
    return to;
  }
  if (from < to) {
    return index > from && index <= to ? index - 1 : index;
  }
  return index >= to && index < from ? index + 1 : index;
}

/**
 * Rewrites the array index of a key under `prefix` for a move of the element at
 * `from` to `to` (`foo[2].bar` -> `foo[0].bar` when index 2 is moved to 0). Keys
 * outside the array, or outside the range the move spans, are returned
 * unchanged. The rewrite is a permutation of the indices, so no two keys are
 * mapped onto one.
 */
export function rewriteKeyAfterMove(
  key: string,
  prefix: string,
  from: number,
  to: number
): string {
  const open = prefix + '[';
  if (key.startsWith(open)) {
    const match = key.slice(open.length).match(/^(\d+)(\].*$)/);
    if (match) {
      const keyIndex = parseInt(match[1], 10);
      const movedIndex = indexAfterMove(keyIndex, from, to);
      if (movedIndex !== keyIndex) {
        return prefix + '[' + movedIndex.toString() + match[2];
      }
    }
  }
  return key;
}

/**
 * Whether `key` names a setting inside the subtree at `keyPrefix` - the setting
 * itself, a member of the group it opens, or an element of the array it names.
 * The empty prefix is the whole tree.
 */
export function isKeyUnder(key: string, keyPrefix: string): boolean {
  return (
    keyPrefix === '' ||
    key === keyPrefix ||
    key.startsWith(keyPrefix + '.') ||
    key.startsWith(keyPrefix + '[')
  );
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

/**
 * Empties the array at `prefix`, dropping every element it holds along with any
 * pending row under it - including the array's own pending row, so what is left
 * is the array as an untouched empty one, not an empty one with a row already
 * asked for.
 */
function applyRemoveAllArrayItems(working: UiWorking, prefix: string): UiWorking {
  const draft = Object.fromEntries(
    Object.entries(working.draft).filter(([key]) => indexAtPrefix(key, prefix) === null)
  );

  const arrayMaxIndex = Object.fromEntries(
    Object.entries(working.arrayMaxIndex).filter(([key]) => !isKeyUnder(key, prefix))
  );

  return { mode: 'ui', draft, arrayMaxIndex };
}

/**
 * Moves the element at `from` to `to` within the array at `prefix`, carrying
 * everything under it - the fields of an object row, the rows of an array
 * nested in one, and the pending rows tracked for either.
 */
function applyMoveArrayItem(
  working: UiWorking,
  prefix: string,
  from: number,
  to: number
): UiWorking {
  const draft = Object.fromEntries(
    Object.entries(working.draft).map(([key, value]) => [
      rewriteKeyAfterMove(key, prefix, from, to),
      value,
    ])
  );

  const arrayMaxIndex = Object.fromEntries(
    Object.entries(working.arrayMaxIndex).map(([key, value]) => [
      rewriteKeyAfterMove(key, prefix, from, to),
      value,
    ])
  );

  return { mode: 'ui', draft, arrayMaxIndex };
}

/**
 * Puts the subtree at `keyPrefix` back to the defaults it is given, replacing
 * whatever the draft holds there rather than merging over it, so a setting the
 * defaults do not name (an array element beyond the declared length, a key a mod
 * has since dropped) does not survive the reset. Rows added but left empty are
 * dropped with it.
 */
function applyResetSetting(
  working: UiWorking,
  keyPrefix: string,
  defaults: ModSettings
): UiWorking {
  const draft = Object.fromEntries(
    Object.entries(working.draft)
      .filter(([key]) => !isKeyUnder(key, keyPrefix))
      .concat(Object.entries(defaults).filter(([key]) => isKeyUnder(key, keyPrefix)))
  );

  const arrayMaxIndex = Object.fromEntries(
    Object.entries(working.arrayMaxIndex).filter(([key]) => !isKeyUnder(key, keyPrefix))
  );

  return { mode: 'ui', draft, arrayMaxIndex };
}

// ============================================================================
// Dirtiness
// ============================================================================

/**
 * How a settings map is put in the one form its values read in, so that two maps
 * differing only in how the same values are spelled compare equal. Injected,
 * since telling one spelling of a value from another means knowing the type the
 * mod declares it with, and this module holds no schema.
 */
export type SettingsCanonicalizer = (settings: ModSettings) => ModSettings;

/**
 * Structural equality of two flat settings maps: same keys, same scalar value
 * per key. Values are `string | number` and `===` is exact, so the maps compared
 * have to be canonical ones - raw, a number 0 and a string '0' are unequal here.
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

/**
 * Whether the draft differs from the saved baseline anywhere under `keyPrefix` -
 * that is, whether saving would change something there. Same comparison
 * `settingsEqual` makes, narrowed to one subtree, so a row reads as unsaved on
 * exactly the terms that make the form dirty - and over canonical maps, for the
 * same reason.
 */
export function isSubtreeChanged(
  draft: ModSettings,
  saved: ModSettings,
  keyPrefix: string
): boolean {
  const draftKeys = Object.keys(draft).filter((key) => isKeyUnder(key, keyPrefix));
  const savedKeys = Object.keys(saved).filter((key) => isKeyUnder(key, keyPrefix));

  return (
    draftKeys.length !== savedKeys.length ||
    draftKeys.some(
      (key) => !Object.prototype.hasOwnProperty.call(saved, key) || draft[key] !== saved[key]
    )
  );
}

export function isDirty(state: EditorState, canonical: SettingsCanonicalizer): boolean {
  if (state.status !== 'ready') {
    return false;
  }

  const { working, saved } = state;
  if (working.mode === 'yaml') {
    // Dirty if the buffer was hand-edited, or if it was seeded from a draft
    // that already differed from the saved baseline (e.g. switching to YAML
    // with unsaved form changes). The unedited buffer round-trips to
    // sourceDraft, so comparing that draft to saved is the content check.
    return working.edited || !settingsEqual(canonical(working.sourceDraft), canonical(saved));
  }

  // An added-but-empty array row (tracked in arrayMaxIndex for rendering)
  // materializes no keys and persists nothing, so it is not a change on its
  // own; dirtiness comes only from values that differ from the saved baseline.
  return !settingsEqual(canonical(working.draft), canonical(saved));
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

    case 'removeAllArrayItems':
      if (working.mode !== 'ui') {
        return state;
      }
      return { ...state, working: applyRemoveAllArrayItems(working, action.prefix) };

    case 'moveArrayItem':
      if (working.mode !== 'ui' || action.from === action.to) {
        return state;
      }
      return {
        ...state,
        working: applyMoveArrayItem(working, action.prefix, action.from, action.to),
      };

    case 'resetSetting':
      if (working.mode !== 'ui') {
        return state;
      }
      return {
        ...state,
        working: applyResetSetting(working, action.keyPrefix, action.defaults),
      };

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
  rewriteKeyAfterMove,
  isKeyUnder,
};
