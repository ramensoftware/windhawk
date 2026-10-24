/**
 * The values a settings map holds, as a mod reads them.
 *
 * A value has more than one spelling in the store: a setting nobody has touched
 * has no key at all, one cleared in the form has an empty key, and a number or a
 * boolean can arrive as either the scalar or a string of it. None of those are
 * differences a mod sees, or the form draws - so comparing two maps key by key
 * would call a draft changed over a value that did not move.
 *
 * Each key is read against the setting the schema declares at it, found by
 * following the key down the schema one segment at a time (`a.b` a member of a
 * group, `a[2]` an element, `a[2].b` a member of a row). Pure: no React, i18n,
 * storage or IPC.
 */

import {
  type InitialSettings,
  type InitialSettingsValue,
} from '@app/webviewIPCMessages';
import {
  describeSetting,
  type ModSettings,
  parseFloatText,
  parseIntLax,
  SettingType,
} from './yamlConverter';

// A setting of one value: what a key can be read as.
type Leaf = {
  kind: SettingType.Boolean | SettingType.Number | SettingType.String;
  float: boolean;
};

// What the schema declares at one segment of a key: a setting of one value, a
// group of members, an array of values, or an array of rows holding members.
type SchemaNode =
  | { kind: 'value'; leaf: Leaf }
  | { kind: 'group'; members: SchemaGroup }
  | { kind: 'values'; leaf: Leaf }
  | { kind: 'rows'; members: SchemaGroup };

type SchemaGroup = Map<string, SchemaNode>;

// An element's subscript at the start of what is left of a key, its index
// spelled the one way an element key spells it: no sign, no leading zero.
const ELEMENT_SUBSCRIPT = /^\[(?:0|[1-9]\d*)\]/;

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
  return canonicalize(settings, schemaGroup(initialSettings), true);
}

/**
 * `canonicalSettings` against one schema, answering a map it was asked about
 * before with the form it gave then. It knows a map by identity, so it holds
 * only for maps that are replaced rather than changed in place.
 */
export function canonicalizerFor(
  initialSettings: InitialSettings
): (settings: ModSettings) => ModSettings {
  const schema = schemaGroup(initialSettings);
  const known = new WeakMap<ModSettings, ModSettings>();
  return (settings) => {
    let canonical = known.get(settings);
    if (!canonical) {
      canonical = canonicalize(settings, schema, true);
      known.set(settings, canonical);
    }
    return canonical;
  };
}

/**
 * `canonicalSettings` without the keys the schema does not describe: the values
 * a mod reads, and nothing else.
 */
export function describedSettings(
  settings: ModSettings,
  initialSettings: InitialSettings
): ModSettings {
  return canonicalize(settings, schemaGroup(initialSettings), false);
}

function canonicalize(
  settings: ModSettings,
  schema: SchemaGroup,
  keepUndescribed: boolean
): ModSettings {
  const canonical: ModSettings = {};

  for (const [key, value] of Object.entries(settings)) {
    const leaf = leafAt(schema, key);
    if (!leaf) {
      if (keepUndescribed) {
        canonical[key] = value;
      }
      continue;
    }

    const read = readValue(leaf, value);
    if (read !== 0 && read !== '') {
      canonical[key] = read;
    }
  }

  return canonical;
}

function schemaGroup(items: InitialSettings): SchemaGroup {
  return new Map(items.map((item) => [item.key, schemaNode(item.value, !!item.float)]));
}

// `float` is the item's `$float` flag. It reaches the item's own values only:
// the members of a group or of a row carry flags of their own.
function schemaNode(value: InitialSettingsValue, float: boolean): SchemaNode {
  const descriptor = describeSetting(value);

  switch (descriptor.kind) {
    case SettingType.Boolean:
    case SettingType.Number:
    case SettingType.String:
      return { kind: 'value', leaf: { kind: descriptor.kind, float } };

    case SettingType.NestedObject:
      return { kind: 'group', members: schemaGroup(descriptor.children) };

    case SettingType.NumberArray:
      return { kind: 'values', leaf: { kind: SettingType.Number, float } };

    case SettingType.StringArray:
      return { kind: 'values', leaf: { kind: SettingType.String, float } };

    case SettingType.ObjectArray:
      return { kind: 'rows', members: schemaGroup(descriptor.children) };
  }
}

/**
 * The setting of one value `key` names, or undefined for a key the schema does
 * not describe: one naming nothing it declares, or shaped other than the way it
 * declares it - a subscript on a setting that is not an array, a group or a row
 * named without a member, an index spelled other than an element key spells it.
 */
function leafAt(schema: SchemaGroup, key: string): Leaf | undefined {
  let members = schema;
  let rest = key;

  for (;;) {
    const nameEnd = rest.search(/[.[]/);
    const node = members.get(nameEnd === -1 ? rest : rest.slice(0, nameEnd));
    if (!node) {
      return undefined;
    }
    rest = nameEnd === -1 ? '' : rest.slice(nameEnd);

    if (node.kind === 'values' || node.kind === 'rows') {
      const subscript = ELEMENT_SUBSCRIPT.exec(rest);
      if (!subscript) {
        return undefined;
      }
      rest = rest.slice(subscript[0].length);
    }

    if (node.kind === 'value' || node.kind === 'values') {
      return rest === '' ? node.leaf : undefined;
    }

    if (!rest.startsWith('.')) {
      return undefined;
    }
    members = node.members;
    rest = rest.slice(1);
  }
}

// A stored value as the setting it is declared as reads it.
function readValue(leaf: Leaf, value: string | number): string | number {
  switch (leaf.kind) {
    case SettingType.Boolean:
      return parseIntLax(value) ? 1 : 0;

    case SettingType.Number:
      return parseIntLax(value);

    case SettingType.String:
      return leaf.float ? canonicalFloat(value) : value.toString();
  }
}

/**
 * A `$float` setting as a mod reads it: the number its text spells, so "1",
 * "1.0" and a stored integer 1 are one value. Text spelling no number stays
 * text - the empty text of an unset setting, which is the zero a mod parses it
 * as and the zero the map leaves out, and anything else, equal only to itself.
 */
function canonicalFloat(raw: string | number): string | number {
  return parseFloatText(raw) ?? raw.toString();
}
