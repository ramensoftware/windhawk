/**
 * The mod-declared defaults of a settings schema, and how a draft compares to
 * them.
 *
 * A mod declares every setting with a value in its settings block; that value is
 * the default the form marks a setting against and reverts it to. It is
 * `describeSetting(value).value` - not the descriptor's `defaultValue`, which is
 * the type's zero (false, 0, '').
 *
 * The walkers below mirror the shape of the settings tree, so the flat keys they
 * produce match the ones the editor holds a draft under (`a.b`, `a[2]`,
 * `a[2].b`). Pure: no React, i18n, storage or IPC.
 */

import {
  type InitialSettings,
  type InitialSettingsValue,
} from '@app/webviewIPCMessages';
import { materializedMaxIndex } from './editorState';
import { describeSetting, type ModSettings, parseIntLax, SettingType } from './yamlConverter';

/**
 * The declared defaults under `keyPrefix`, in the form the store holds: a
 * boolean is a 0/1 integer, the same way the YAML converter stores one.
 */
export function flattenSettingDefaults(
  value: InitialSettingsValue,
  keyPrefix: string
): ModSettings {
  const descriptor = describeSetting(value);

  switch (descriptor.kind) {
    case SettingType.Boolean:
      return { [keyPrefix]: descriptor.value ? 1 : 0 };

    case SettingType.Number:
    case SettingType.String:
      return { [keyPrefix]: descriptor.value };

    case SettingType.NestedObject:
      return flattenGroupDefaults(descriptor.value, keyPrefix + '.');

    case SettingType.NumberArray:
    case SettingType.StringArray:
      return Object.fromEntries(
        descriptor.value.map((item, index) => [`${keyPrefix}[${index}]`, item])
      );

    case SettingType.ObjectArray:
      return Object.assign(
        {},
        ...descriptor.value.map((row, index) =>
          flattenGroupDefaults(row, `${keyPrefix}[${index}].`)
        )
      );
  }
}

function flattenGroupDefaults(items: InitialSettings, keyPrefix: string): ModSettings {
  return Object.assign(
    {},
    ...items.map((item) => flattenSettingDefaults(item.value, keyPrefix + item.key))
  );
}

/**
 * The declared defaults of a whole schema, keyed the way a draft is.
 */
export function flattenAllDefaults(initialSettings: InitialSettings): ModSettings {
  return flattenGroupDefaults(initialSettings, '');
}

/**
 * Whether the draft differs from the declared default anywhere under
 * `keyPrefix`. A group or an array is modified when any of its descendants is,
 * and an array also when its length differs from the declared one.
 *
 * The comparison is made per setting type rather than between two flat maps,
 * which is what keeps a stored '0' against a declared 0, a key the draft never
 * materialized, and a string cleared to '' from reading as differences they are
 * not.
 */
export function isSettingModified(
  draft: ModSettings,
  value: InitialSettingsValue,
  keyPrefix: string
): boolean {
  const descriptor = describeSetting(value);

  switch (descriptor.kind) {
    case SettingType.Boolean:
      return !!parseIntLax(draft[keyPrefix]) !== descriptor.value;

    case SettingType.Number:
      return parseIntLax(draft[keyPrefix]) !== descriptor.value;

    case SettingType.String:
      return (draft[keyPrefix] ?? '').toString() !== descriptor.value;

    case SettingType.NestedObject:
      return isGroupModified(draft, descriptor.value, keyPrefix + '.');

    case SettingType.NumberArray:
    case SettingType.StringArray:
      return (
        arrayLength(draft, keyPrefix) !== descriptor.value.length ||
        descriptor.value.some((item, index) =>
          isSettingModified(draft, item, `${keyPrefix}[${index}]`)
        )
      );

    case SettingType.ObjectArray:
      return (
        arrayLength(draft, keyPrefix) !== descriptor.value.length ||
        descriptor.value.some((row, index) =>
          isGroupModified(draft, row, `${keyPrefix}[${index}].`)
        )
      );
  }
}

function isGroupModified(
  draft: ModSettings,
  items: InitialSettings,
  keyPrefix: string
): boolean {
  return items.some((item) => isSettingModified(draft, item.value, keyPrefix + item.key));
}

/**
 * The number of array elements the draft materialized under `keyPrefix`. A row
 * the user added but left empty materializes no key and so does not count, the
 * same way it does not make the form dirty.
 */
function arrayLength(draft: ModSettings, keyPrefix: string): number {
  return materializedMaxIndex(draft, keyPrefix) + 1;
}

/**
 * A one-line rendering of a declared default, or null for a group or an array,
 * which has no single value to name. A boolean reads as the true/false a mod
 * declares it with, matching how the YAML mode renders one. An empty string has
 * nothing to show, so it reads as no value rather than as blank text.
 */
export function formatDefaultValue(value: InitialSettingsValue): string | null {
  const descriptor = describeSetting(value);

  switch (descriptor.kind) {
    case SettingType.Boolean:
      return descriptor.value ? 'true' : 'false';

    case SettingType.Number:
      return descriptor.value.toString();

    case SettingType.String:
      return descriptor.value === '' ? null : descriptor.value;

    default:
      return null;
  }
}
