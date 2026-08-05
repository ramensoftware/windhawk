/**
 * The values a settings map holds, as a mod reads them.
 *
 * A value has more than one spelling in the store: a setting nobody has touched
 * has no key at all, one cleared in the form has an empty key, and a number or a
 * boolean can arrive as either the scalar or a string of it. None of those are
 * differences a mod sees, or the form draws - so comparing two maps key by key
 * would call a draft changed over a value that did not move.
 *
 * The walkers below mirror the shape of the settings tree, the same way the
 * defaults walkers do, so the flat keys they produce match the ones the editor
 * holds a draft under (`a.b`, `a[2]`, `a[2].b`). Pure: no React, i18n, storage
 * or IPC.
 */

import {
  type InitialSettings,
  type InitialSettingsValue,
} from '@app/webviewIPCMessages';
import { materializedMaxIndex } from './editorState';
import { describeSetting, type ModSettings, parseIntLax, SettingType } from './yamlConverter';

// What the walk produces: the canonical map, and the keys the schema accounted
// for - the rest of the map is carried over untouched.
type CanonicalAccumulator = {
  canonical: ModSettings;
  described: Set<string>;
};

/**
 * `settings` with every value the schema describes normalized to the type it is
 * stored as, and left out when it is that type's zero - an empty string, 0, a
 * false boolean - which is what a mod reads a key that is not there as. A key
 * the schema does not describe is carried over as it is: there is no type to
 * read it against, and a save still writes it.
 *
 * Two settings maps configure a mod identically exactly when their canonical
 * forms are equal, which is what an unsaved edit is judged by.
 */
export function canonicalSettings(
  settings: ModSettings,
  initialSettings: InitialSettings
): ModSettings {
  const accumulator: CanonicalAccumulator = { canonical: {}, described: new Set() };
  canonicalizeGroup(settings, initialSettings, '', accumulator);

  const { canonical, described } = accumulator;
  for (const [key, value] of Object.entries(settings)) {
    if (!described.has(key)) {
      canonical[key] = value;
    }
  }

  return canonical;
}

function canonicalizeGroup(
  settings: ModSettings,
  items: InitialSettings,
  keyPrefix: string,
  accumulator: CanonicalAccumulator
): void {
  for (const item of items) {
    canonicalizeSetting(settings, item.value, keyPrefix + item.key, accumulator);
  }
}

function canonicalizeSetting(
  settings: ModSettings,
  value: InitialSettingsValue,
  keyPrefix: string,
  accumulator: CanonicalAccumulator
): void {
  const descriptor = describeSetting(value);

  switch (descriptor.kind) {
    case SettingType.Boolean:
      setCanonical(accumulator, keyPrefix, parseIntLax(settings[keyPrefix]) ? 1 : 0);
      break;

    case SettingType.Number:
      setCanonical(accumulator, keyPrefix, parseIntLax(settings[keyPrefix]));
      break;

    case SettingType.String:
      setCanonical(accumulator, keyPrefix, (settings[keyPrefix] ?? '').toString());
      break;

    case SettingType.NestedObject:
      canonicalizeGroup(settings, descriptor.value, keyPrefix + '.', accumulator);
      break;

    case SettingType.NumberArray:
    case SettingType.StringArray:
      forEachElement(settings, keyPrefix, (elementKey) =>
        canonicalizeSetting(settings, descriptor.value[0], elementKey, accumulator)
      );
      break;

    case SettingType.ObjectArray:
      forEachElement(settings, keyPrefix, (elementKey) =>
        canonicalizeGroup(settings, descriptor.children, elementKey + '.', accumulator)
      );
      break;
  }
}

/**
 * Records what the schema makes of one key: the key is accounted for either way,
 * and it is carried only when it holds something, the type's zero being how an
 * unset setting reads.
 */
function setCanonical(
  accumulator: CanonicalAccumulator,
  key: string,
  value: string | number
): void {
  accumulator.described.add(key);
  if (value !== 0 && value !== '') {
    accumulator.canonical[key] = value;
  }
}

/**
 * Runs over the array elements `settings` materialized under `keyPrefix`. A
 * further element holds nothing, which is what it would canonicalize to anyway,
 * so the schema's own length is not what the walk follows - it is one template
 * for an array of any length.
 */
function forEachElement(
  settings: ModSettings,
  keyPrefix: string,
  visit: (elementKey: string) => void
): void {
  const maxIndex = materializedMaxIndex(settings, keyPrefix);
  for (let index = 0; index <= maxIndex; index++) {
    visit(`${keyPrefix}[${index}]`);
  }
}
