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
import { settingsEqual } from './editorState';
import { canonicalSubtree } from './settingValues';
import { describeSetting, type ModSettings, SettingType } from './yamlConverter';

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
 * `keyPrefix`.
 *
 * The comparison is of the two sides canonicalized, i.e. of the values a mod
 * reads rather than of the keys a store holds - which is what keeps a stored '0'
 * against a declared 0, a key the draft never materialized, and an array row
 * holding nothing from reading as differences they are not. It is the comparison
 * an unsaved edit is judged by, made against the mod's defaults instead of the
 * saved settings, so a row cannot be marked as away from its default and as
 * holding nothing to save at once.
 */
export function isSettingModified(
  draft: ModSettings,
  value: InitialSettingsValue,
  keyPrefix: string
): boolean {
  return !settingsEqual(
    canonicalSubtree(draft, value, keyPrefix),
    canonicalSubtree(flattenSettingDefaults(value, keyPrefix), value, keyPrefix)
  );
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
