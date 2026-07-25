/**
 * YAML Schema Validation and Conversion Utilities for Mod Settings
 *
 * This module provides:
 * - Type definitions for mod settings
 * - YAML schema validation against InitialSettings
 * - Bidirectional conversion between flat settings and nested YAML
 */

import {
  type InitialSettingItem,
  type InitialSettings,
  type InitialSettingsValue,
} from '@app/webviewIPCMessages';
import * as yaml from 'js-yaml';
import type { useTranslation } from 'react-i18next';

// ============================================================================
// Type Definitions
// ============================================================================

export type ModSettings = Record<string, string | number>;

export type NestedValue = string | number | NestedSettings | (string | number | NestedSettings)[];

export interface NestedSettings {
  [key: string]: NestedValue;
}

export interface TypeMismatchError {
  key: string;
  expected: string;
  actual: string;
}

// ============================================================================
// Setting Type Descriptors
// ============================================================================

export enum SettingType {
  Boolean = 'boolean',
  Number = 'number',
  String = 'string',
  NestedObject = 'nested-object',
  NumberArray = 'number-array',
  StringArray = 'string-array',
  ObjectArray = 'object-array',
}

type BooleanDescriptor = {
  kind: SettingType.Boolean;
  value: boolean;
  defaultValue: number;
};

type NumberDescriptor = {
  kind: SettingType.Number;
  value: number;
  defaultValue: number;
};

type StringDescriptor = {
  kind: SettingType.String;
  value: string;
  defaultValue: string;
};

type NestedDescriptor = {
  kind: SettingType.NestedObject;
  value: InitialSettings;
  children: InitialSettings;
};

type NumberArrayDescriptor = {
  kind: SettingType.NumberArray;
  value: number[];
  defaultValue: number;
};

type StringArrayDescriptor = {
  kind: SettingType.StringArray;
  value: string[];
  defaultValue: string;
};

type ObjectArrayDescriptor = {
  kind: SettingType.ObjectArray;
  value: InitialSettings[];
  children: InitialSettings;
};

type SettingDescriptor =
  | BooleanDescriptor
  | NumberDescriptor
  | StringDescriptor
  | NestedDescriptor
  | NumberArrayDescriptor
  | StringArrayDescriptor
  | ObjectArrayDescriptor;

// ============================================================================
// Type Guard Functions
// ============================================================================

function isInitialSettingItem(value: unknown): value is InitialSettingItem {
  if (typeof value !== 'object' || value === null) {
    return false;
  }

  const record = value as Record<string, unknown>;
  return typeof record['key'] === 'string' && 'value' in record;
}

function isInitialSettingsArray(value: unknown): value is InitialSettings {
  return Array.isArray(value) && value.every(isInitialSettingItem);
}

function isInitialSettingsCollection(value: unknown[]): value is InitialSettings[] {
  return value.every(isInitialSettingsArray);
}

function isNumberArrayValue(value: unknown[]): value is number[] {
  return value.every(item => typeof item === 'number');
}

function isStringArrayValue(value: unknown[]): value is string[] {
  return value.every(item => typeof item === 'string');
}

// ============================================================================
// Setting Descriptor Functions
// ============================================================================

export function describeSetting(value: InitialSettingsValue): SettingDescriptor {
  if (typeof value === 'boolean') {
    return { kind: SettingType.Boolean, value, defaultValue: 0 };
  }

  if (typeof value === 'number') {
    return { kind: SettingType.Number, value, defaultValue: 0 };
  }

  if (typeof value === 'string') {
    return { kind: SettingType.String, value, defaultValue: '' };
  }

  if (!Array.isArray(value) || value.length === 0) {
    throw new Error('Initial settings arrays must contain at least one template entry.');
  }

  const arrayValue: unknown[] = value;

  if (isInitialSettingsCollection(arrayValue)) {
    const [first] = arrayValue;
    if (first.length === 0) {
      throw new Error('Invalid object array schema definition.');
    }
    return { kind: SettingType.ObjectArray, value: arrayValue, children: first };
  }

  if (isInitialSettingsArray(arrayValue)) {
    return { kind: SettingType.NestedObject, value: arrayValue, children: arrayValue };
  }

  if (isNumberArrayValue(arrayValue)) {
    return { kind: SettingType.NumberArray, value: arrayValue, defaultValue: 0 };
  }

  if (isStringArrayValue(arrayValue)) {
    return { kind: SettingType.StringArray, value: arrayValue, defaultValue: '' };
  }

  throw new Error(`Unknown setting type for value: ${JSON.stringify(value)}`);
}

// ============================================================================
// Utility Functions
// ============================================================================

export function parseIntLax(value?: string | number | null) {
  const result = parseInt((value ?? 0).toString(), 10);
  return Number.isNaN(result) ? 0 : result;
}

/**
 * Helper to check if a value is a plain object (not array, not null)
 */
export function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function toNestedSettings(value: unknown): NestedSettings {
  return isPlainObject(value) ? (value as NestedSettings) : {};
}

/**
 * Natural sort comparator for strings with numbers.
 * Compares strings such that "item2" comes before "item10".
 */
export function naturalSort(a: string, b: string): number {
  return a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' });
}

// ============================================================================
// YAML Schema Validation
// ============================================================================

export class YamlSchemaValidator {
  private validKeys: Set<string>;
  private typeSchema: Map<string, string>;

  constructor(initialSettings: InitialSettings) {
    this.validKeys = this.buildValidKeys(initialSettings);
    this.typeSchema = this.buildTypeSchema(initialSettings);
  }

  private buildValidKeys(settings: InitialSettings, prefix = ''): Set<string> {
    const keys = new Set<string>();

    for (const item of settings) {
      const key = prefix ? `${prefix}.${item.key}` : item.key;
      keys.add(key);

      const descriptor = describeSetting(item.value);

      if (descriptor.kind === SettingType.NestedObject || descriptor.kind === SettingType.ObjectArray) {
        const nestedKeys = this.buildValidKeys(descriptor.children, key);
        nestedKeys.forEach(nestedKey => keys.add(nestedKey));
      }
    }

    return keys;
  }

  private buildTypeSchema(settings: InitialSettings, prefix = ''): Map<string, string> {
    const schema = new Map<string, string>();

    for (const item of settings) {
      const key = prefix ? `${prefix}.${item.key}` : item.key;
      const descriptor = describeSetting(item.value);

      switch (descriptor.kind) {
        case SettingType.Boolean:
        case SettingType.Number:
          schema.set(key, 'number');
          break;
        case SettingType.String:
          schema.set(key, 'string');
          break;
        case SettingType.NestedObject:
        case SettingType.ObjectArray: {
          const nestedSchema = this.buildTypeSchema(descriptor.children, key);
          nestedSchema.forEach((type, nestedKey) => schema.set(nestedKey, type));
          break;
        }
        case SettingType.NumberArray:
          schema.set(key, 'number[]');
          break;
        case SettingType.StringArray:
          schema.set(key, 'string[]');
          break;
      }
    }

    return schema;
  }

  validateKeys(nested: NestedSettings, prefix = ''): string | null {
    for (const [key, value] of Object.entries(nested)) {
      const fullKey = prefix ? `${prefix}.${key}` : key;

      // Check validity for this key first
      if (!this.validKeys.has(fullKey)) {
        return fullKey;
      }

      // Then recurse into nested structures
      if (Array.isArray(value)) {
        for (const item of value) {
          if (isPlainObject(item)) {
            const invalidKey = this.validateKeys(item, fullKey);
            if (invalidKey) {
              return invalidKey;
            }
          }
        }
      } else if (isPlainObject(value)) {
        const invalidKey = this.validateKeys(value, fullKey);
        if (invalidKey) {
          return invalidKey;
        }
      }
    }

    return null;
  }

  validateTypes(nested: NestedSettings, prefix = ''): TypeMismatchError | null {
    for (const [key, value] of Object.entries(nested)) {
      const fullKey = prefix ? `${prefix}.${key}` : key;
      const expectedType = this.typeSchema.get(fullKey);

      if (expectedType) {
        const error = this.validateValue(fullKey, value, expectedType);
        if (error) return error;
      } else {
        // Even if key is not in schema, recurse into nested structures
        if (Array.isArray(value)) {
          for (const item of value) {
            if (isPlainObject(item)) {
              const error = this.validateTypes(item, fullKey);
              if (error) return error;
            }
          }
        } else if (isPlainObject(value)) {
          const error = this.validateTypes(value, fullKey);
          if (error) return error;
        }
      }
    }

    return null;
  }

  private validateValue(
    fullKey: string,
    value: NestedValue,
    expectedType: string
  ): TypeMismatchError | null {
    const actualType = this.getActualType(value);

    // Handle array types
    if (expectedType.endsWith('[]')) {
      if (!Array.isArray(value)) {
        return { key: fullKey, expected: 'array', actual: actualType };
      }
      return this.validateArrayElements(fullKey, value, expectedType);
    }

    // Handle primitive types
    if (expectedType !== actualType) {
      return { key: fullKey, expected: expectedType, actual: actualType };
    }

    // Handle nested objects
    if (isPlainObject(value)) {
      return this.validateTypes(value, fullKey);
    }

    return null;
  }

  private getActualType(value: NestedValue): string {
    if (Array.isArray(value)) return 'array';
    return typeof value;
  }

  private validateArrayElements(
    fullKey: string,
    array: NestedValue[],
    expectedType: string
  ): TypeMismatchError | null {
    const elementType = expectedType.replace('[]', '');

    for (let i = 0; i < array.length; i++) {
      const item = array[i];
      const itemKey = `${fullKey}[${i}]`;
      const actualType = this.getActualType(item);

      if (elementType === 'object') {
        if (!isPlainObject(item)) {
          return { key: itemKey, expected: 'object', actual: actualType };
        }
        const typeError = this.validateTypes(item, fullKey);
        if (typeError) return typeError;
      } else if (elementType !== actualType) {
        return { key: itemKey, expected: elementType, actual: actualType };
      }
    }

    return null;
  }
}

// ============================================================================
// YAML Conversion Utilities
// ============================================================================

export class YamlConverter {
  static flatToNested(flatSettings: ModSettings, initialSettings: InitialSettings): NestedSettings {
    const nested: NestedSettings = {};
    const keysToProcess = Object.keys(flatSettings);

    // Filter keys to only include those that match the schema structure
    const validKeys = keysToProcess.filter(key => this.keyMatchesSchemaStructure(key, initialSettings));

    for (const key of validKeys) {
      this.setNestedValue(nested, key, flatSettings[key]);
    }

    return this.normalizeWithSchema(nested, initialSettings);
  }

  /**
   * Check if a key path matches the schema structure.
   * Returns false if:
   * - Key uses array notation [index] where schema defines an object
   * - Key uses object notation .property where schema defines an array
   */
  private static keyMatchesSchemaStructure(key: string, initialSettings: InitialSettings): boolean {
    const parts = this.parseKeyPath(key);
    let currentSettings = initialSettings;

    for (let i = 0; i < parts.length; i++) {
      const { part, index } = parts[i];

      // Find the setting that matches this part
      const setting = currentSettings.find(s => s.key === part);

      if (!setting) {
        // Key not in schema - let validation handle it
        return true;
      }

      const descriptor = describeSetting(setting.value);
      const isArrayPart = index !== undefined;
      const expectsArray =
        descriptor.kind === SettingType.NumberArray ||
        descriptor.kind === SettingType.StringArray ||
        descriptor.kind === SettingType.ObjectArray;

      if (expectsArray !== isArrayPart) {
        return false;
      }

      switch (descriptor.kind) {
        case SettingType.ObjectArray:
        case SettingType.NestedObject:
          currentSettings = descriptor.children;
          break;
        default:
          return true;
      }
    }

    return true;
  }

  private static setNestedValue(nested: NestedSettings, key: string, value: string | number): void {
    const parts = this.parseKeyPath(key);
    let current = nested;

    // Navigate through all parts, creating structure as needed
    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      const isLastPart = i === parts.length - 1;

      if (part.index !== undefined) {
        // Navigate to array by property name
        current[part.part] ??= [];
        const currentArray = current[part.part] as NestedValue[];

        // Set value or navigate to array element
        if (isLastPart) {
          currentArray[part.index] = value;
        } else {
          currentArray[part.index] ??= {};
          current = currentArray[part.index] as NestedSettings;
        }
      } else {
        // Set value or navigate to property
        if (isLastPart) {
          current[part.part] = value;
        } else {
          current[part.part] ??= {};
          current = current[part.part] as NestedSettings;
        }
      }
    }
  }

  /**
   * Parse a key path and track whether each part is from bracket notation.
   * Returns array of {part, index} objects. index is optional.
   * Example: "config.x" -> [{part: 'config'}, {part: 'x'}]
   * Example: "config.42" -> [{part: 'config'}, {part: '42'}]
   * Example: "config[42]" -> [{part: 'config', index: 42}]
   */
  private static parseKeyPath(key: string): Array<{ part: string; index?: number }> {
    const parts: Array<{ part: string; index?: number }> = [];
    let remaining = key;

    while (remaining) {
      // Match property name with optional array index: word or word[123]
      const match = remaining.match(/^([^.[]+)(?:\[(\d+)\])?\.?(.*)/);
      if (!match) {
        break;
      }

      const part: { part: string; index?: number } = { part: match[1] };
      if (match[2] !== undefined) {
        part.index = parseInt(match[2], 10);
      }

      parts.push(part);

      remaining = match[3];
    }

    return parts;
  }

  /**
   * Combines provided values with schema metadata: orders keys, applies
   * defaults, and coerces to schema types.
   */
  private static normalizeWithSchema(target: NestedSettings, schema: InitialSettings): NestedSettings {
    const ordered: NestedSettings = {};
    const remainingKeys = new Set(Object.keys(target));

    for (const item of schema) {
      const { key } = item;
      const descriptor = describeSetting(item.value);
      const existingValue = target[key];

      switch (descriptor.kind) {
        case SettingType.Boolean:
        case SettingType.Number:
        case SettingType.String:
          ordered[key] = this.normalizePrimitiveValue(existingValue, descriptor);
          break;
        case SettingType.NestedObject:
          ordered[key] = this.normalizeNestedObject(existingValue, descriptor.children);
          break;
        case SettingType.ObjectArray:
          ordered[key] = this.normalizeObjectArray(existingValue, descriptor.children);
          break;
        case SettingType.NumberArray:
          ordered[key] = this.normalizePrimitiveArray(existingValue, descriptor.defaultValue, this.isNumberValue);
          break;
        case SettingType.StringArray:
          ordered[key] = this.normalizePrimitiveArray(existingValue, descriptor.defaultValue, this.isStringValue);
          break;
      }

      remainingKeys.delete(key);
    }

    if (remainingKeys.size > 0) {
      const extras = Array.from(remainingKeys).sort(naturalSort);
      for (const key of extras) {
        ordered[key] = target[key];
      }
    }

    return ordered;
  }

  private static highestDefinedIndex(array: unknown[]): number {
    for (let i = array.length - 1; i >= 0; i--) {
      if (array[i] !== undefined) {
        return i;
      }
    }
    return -1;
  }

  private static normalizeNestedObject(value: NestedValue | undefined, schema: InitialSettings): NestedSettings {
    return this.normalizeWithSchema(toNestedSettings(value), schema);
  }

  private static normalizeObjectArray(value: NestedValue | undefined, schema: InitialSettings): NestedSettings[] {
    const existingArray = Array.isArray(value) ? value : [];
    const highestIndex = Math.max(this.highestDefinedIndex(existingArray), 0);
    const result: NestedSettings[] = [];

    for (let index = 0; index <= highestIndex; index += 1) {
      result[index] = this.normalizeWithSchema(toNestedSettings(existingArray[index]), schema);
    }

    return result;
  }

  private static normalizePrimitiveArray<T extends string | number>(
    value: NestedValue | undefined,
    defaultValue: T,
    guard: (candidate: unknown) => candidate is T
  ): T[] {
    const existingArray = Array.isArray(value) ? value : [];
    const highestIndex = Math.max(this.highestDefinedIndex(existingArray), 0);
    const result: T[] = [];

    for (let index = 0; index <= highestIndex; index += 1) {
      const candidate = existingArray[index];
      result[index] = guard(candidate) ? candidate : defaultValue;
    }

    return result;
  }

  private static isNumberValue(value: unknown): value is number {
    return typeof value === 'number';
  }

  private static isStringValue(value: unknown): value is string {
    return typeof value === 'string';
  }

  private static normalizePrimitiveValue(
    value: NestedValue | undefined,
    descriptor: BooleanDescriptor | NumberDescriptor | StringDescriptor
  ): string | number {
    if (descriptor.kind === SettingType.Boolean) {
      return this.normalizeBooleanValue(value, descriptor.defaultValue);
    }

    if (descriptor.kind === SettingType.Number) {
      return this.normalizeNumberValue(value, descriptor.defaultValue);
    }

    return this.normalizeStringValue(value, descriptor.defaultValue);
  }

  private static normalizeBooleanValue(
    value: NestedValue | undefined,
    defaultValue: number
  ): number {
    if (value === undefined) {
      return defaultValue;
    }

    if (typeof value === 'number') {
      return value ? 1 : 0;
    }

    if (typeof value === 'string') {
      return parseIntLax(value) ? 1 : 0;
    }

    return defaultValue;
  }

  private static normalizeNumberValue(
    value: NestedValue | undefined,
    defaultValue: number
  ): number {
    if (value === undefined) {
      return defaultValue;
    }

    if (typeof value === 'number') {
      return value;
    }

    if (typeof value === 'string') {
      return parseIntLax(value);
    }

    return defaultValue;
  }

  private static normalizeStringValue(
    value: NestedValue | undefined,
    defaultValue: string
  ): string {
    if (value === undefined) {
      return defaultValue;
    }

    if (typeof value === 'string') {
      return value;
    }

    if (typeof value === 'number') {
      return value.toString();
    }

    return defaultValue;
  }

  static nestedToFlat(nested: NestedValue, prefix = ''): ModSettings {
    const flat: ModSettings = {};

    if (Array.isArray(nested)) {
      nested.forEach((item, index) => {
        const key = `${prefix}[${index}]`;
        Object.assign(flat, isPlainObject(item)
          ? this.nestedToFlat(item, key)
          : { [key]: item }
        );
      });
    } else {
      for (const [key, value] of Object.entries(nested)) {
        const fullKey = prefix ? `${prefix}.${key}` : key;

        if (Array.isArray(value)) {
          value.forEach((item, index) => {
            const arrayKey = `${fullKey}[${index}]`;
            Object.assign(flat, isPlainObject(item)
              ? this.nestedToFlat(item as NestedSettings, arrayKey)
              : { [arrayKey]: item }
            );
          });
        } else if (isPlainObject(value)) {
          Object.assign(flat, this.nestedToFlat(value as NestedSettings, fullKey));
        } else {
          flat[fullKey] = value;
        }
      }
    }

    return flat;
  }

  static removeEmptyValues(value: NestedValue): NestedValue {
    if (Array.isArray(value)) {
      return this.cleanArray(value);
    }

    if (isPlainObject(value)) {
      return this.cleanObject(value);
    }

    return value;
  }

  private static cleanArray(array: (string | number | NestedSettings)[]): (string | number | NestedSettings)[] {
    // Compact a possibly sparse array
    const compacted = Object.values(array);

    // Find the last non-empty index, but skip the first element
    let lastNonEmpty = 0;
    for (let i = compacted.length - 1; i >= 1; i--) {
      const value = compacted[i];
      if (!this.isEmptyValue(value)) {
        lastNonEmpty = i;
        break;
      }
    }

    // Trim to last non-empty element, but never remove all elements
    const trimmed = compacted.slice(0, lastNonEmpty + 1);

    // Clean nested objects
    const cleaned = trimmed
      .map(value => {
        if (isPlainObject(value)) {
          return this.cleanObject(value);
        }

        return value;
      });

    return cleaned;
  }

  private static cleanObject(obj: NestedSettings): NestedSettings {
    return Object.fromEntries(
      Object.entries(obj)
        .map(([key, val]) => [key, this.removeEmptyValues(val)])
    );
  }

  private static isEmptyValue(value: NestedValue): boolean {
    if (Array.isArray(value)) {
      return value.every(v => this.isEmptyValue(v));
    }

    if (isPlainObject(value)) {
      return Object.values(value).every(v => this.isEmptyValue(v));
    }

    return value === '' || value === 0;
  }

  static toYaml(settings: ModSettings, initialSettings: InitialSettings): string {
    try {
      const nested = this.flatToNested(settings, initialSettings);
      const cleaned = this.removeEmptyValues(nested);
      const yamlText = yaml.dump(cleaned, {
        indent: 2,
        lineWidth: -1,
        noRefs: true,
        sortKeys: false,
      });
      return yamlText.trim() === '{}' ? '' : yamlText;
    } catch (error) {
      console.error('Error converting settings to YAML:', error);
      return '';
    }
  }

  static fromYaml(
    yamlString: string,
    validator: YamlSchemaValidator,
    t: ReturnType<typeof useTranslation>['t'],
  ): { settings: ModSettings | null; error: string | null } {
    if (!yamlString.trim()) {
      return { settings: {}, error: null };
    }

    try {
      const parsed = yaml.load(yamlString);

      // Validate structure
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
        return { settings: null, error: t('modDetails.settings.yamlInvalid') };
      }

      // Validate keys
      const invalidKey = validator.validateKeys(parsed as NestedSettings);
      if (invalidKey) {
        return {
          settings: null,
          error: t('modDetails.settings.yamlInvalidKey', { key: invalidKey })
        };
      }

      // Validate types
      const typeError = validator.validateTypes(parsed as NestedSettings);
      if (typeError) {
        return {
          settings: null,
          error: t('modDetails.settings.yamlTypeMismatch', {
            key: typeError.key,
            expected: typeError.expected,
            actual: typeError.actual
          })
        };
      }

      return { settings: this.nestedToFlat(parsed as NestedSettings), error: null };
    } catch (error) {
      return {
        settings: null,
        error: t('modDetails.settings.yamlParseError', {
          error: error instanceof Error ? error.message : String(error)
        })
      };
    }
  }
}

// ============================================================================
// Exported for Testing
// ============================================================================

// Types exported for testing only
export type typesForTesting = {
  ModSettings: ModSettings;
  NestedValue: NestedValue;
  NestedSettings: NestedSettings;
  InitialSettings: InitialSettings;
  InitialSettingItem: InitialSettingItem;
  TypeMismatchError: TypeMismatchError;
};

// Exported for testing only
export const exportedForTesting = {
  // Types
  SettingType,
  // Helper functions
  isPlainObject,
  naturalSort,
  // Classes
  YamlSchemaValidator,
  YamlConverter,
};
