import { faCaretDown } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import Editor, { loader } from '@monaco-editor/react';
import { Button, Card, ConfigProvider, List, message, Modal, Select, Switch } from 'antd';
import * as yaml from 'js-yaml';
import * as monaco from 'monaco-editor/esm/vs/editor/editor.api';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useBlocker } from 'react-router-dom';
import styled from 'styled-components';
import { useEventListener } from 'usehooks-ts';
import { DropdownModal, dropdownModalDismissed, InputNumberWithContextMenu, InputWithContextMenu, SelectModal } from '../components/InputWithContextMenu';
import { useGetModSettings, useSetModSettings } from '../webviewIPC';
import { mockModSettings } from './mockData';

// Configure Monaco Editor to use local npm package instead of CDN.
loader.config({ monaco });

const SettingsWrapper = styled.div`
  // If an object list (with split={false}) is nested inside an array list (without split={false}),
  // the array list's CSS is applied to the object list's CSS, forcing the split style.
  // This CSS rule explicitly removes the split from object lists.
  .ant-list:not(.ant-list-split) > div > div > ul > li.ant-list-item {
    border-bottom: none;
  }

  padding-top: 12px;
  padding-bottom: 12px;
`;

const SettingInputNumber = styled(InputNumberWithContextMenu)`
  width: 100%;
  max-width: 130px;

  // Remove default VSCode focus highlighting color.
  input:focus {
    outline: none !important;
  }
`;

const SettingSelect = styled(SelectModal)`
  width: 100%;
`;

const SettingsCard = styled(Card)`
  width: 100%;
`;

const ArraySettingsItemWrapper = styled.div`
  display: flex;
  gap: 12px;
`;

const ArraySettingsDropdownOptionsButton = styled(Button)`
  padding-inline-start: 10px;
  padding-inline-end: 10px;
`;

const SettingsListItem = styled(List.Item)`
  &:first-child {
    padding-top: 0;
  }

  &:last-child {
    padding-bottom: 0;
  }
`;

const SettingsListItemMeta = styled(List.Item.Meta)`
  .ant-list-item-meta {
    margin-bottom: 8px;
  }

  .ant-list-item-meta-title {
    margin-bottom: 0;
  }

  .ant-list-item-meta-description {
    white-space: pre-line;
  }
`;

const SaveSettingsCard = styled(Card)`
  position: sticky;
  top: 0;
  z-index: 1;
  margin-inline-start: -12px;
  margin-inline-end: -12px;
  margin-top: -12px;
`;

const ActionButtonsWrapper = styled.div`
  display: flex;
  gap: 12px;
`;

const YamlEditorWrapper = styled.div`
  direction: ltr;
  margin-top: 12px;
`;

const YamlErrorContent = styled.div`
  display: inline-block;
  text-align: start;
  font-family: 'Consolas', 'Monaco', 'Courier New', monospace;
  white-space: pre-wrap;
`;

type ModSettings = Record<string, string | number>;

type NestedValue = string | number | NestedSettings | (string | number | NestedSettings)[];

interface NestedSettings {
  [key: string]: NestedValue;
}

type InitialSettings = InitialSettingItem[];

type InitialSettingItem = {
  key: string;
  value: InitialSettingsValue;
  name?: string;
  description?: string;
} & InitialSettingItemExtra;

type InitialSettingItemExtra = {
  options?: Record<string, string>[];
};

type InitialSettingsValue =
  | boolean
  | number
  | string
  | InitialSettings
  | InitialSettingsArrayValue;

type InitialSettingsArrayValue = number[] | string[] | InitialSettings[];

enum SettingType {
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

function describeSetting(value: InitialSettingsValue): SettingDescriptor {
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

function parseIntLax(value?: string | number | null) {
  const result = parseInt((value ?? 0).toString(), 10);
  return Number.isNaN(result) ? 0 : result;
}

/**
 * Formats a YAML error message for display in Ant Design message component.
 * Handles multiline error messages by rendering each line separately.
 */
function formatYamlError(error: string): React.ReactNode {
  const lines = error.split('\n');
  return (
    <YamlErrorContent>
      {lines.map((line, index) => (
        <span key={index}>
          {line}
          {index < lines.length - 1 && <br />}
        </span>
      ))}
    </YamlErrorContent>
  );
}

// ============================================================================
// YAML Schema Validation
// ============================================================================

interface TypeMismatchError {
  key: string;
  expected: string;
  actual: string;
}

/**
 * Helper to check if a value is a plain object (not array, not null)
 */
function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function toNestedSettings(value: unknown): NestedSettings {
  return isPlainObject(value) ? (value as NestedSettings) : {};
}

/**
 * Natural sort comparator for strings with numbers.
 * Compares strings such that "item2" comes before "item10".
 */
function naturalSort(a: string, b: string): number {
  return a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' });
}

class YamlSchemaValidator {
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

class YamlConverter {
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
    const cleaned = trimmed.
      map(value => {
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
// Component Definitions
// ============================================================================

interface BooleanSettingProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
}

function BooleanSetting({ checked, onChange }: BooleanSettingProps) {
  return <Switch checked={checked} onChange={onChange} />;
}

interface StringSettingProps {
  value: string;
  sampleValue: string;
  onChange: (newValue: string) => void;
}

function StringSetting({ value, sampleValue, onChange }: StringSettingProps) {
  const { t } = useTranslation();

  return (
    <InputWithContextMenu
      placeholder={
        sampleValue
          ? t('modDetails.settings.sampleValue') + `: ${sampleValue}`
          : undefined
      }
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

interface SelectSettingProps {
  value: string;
  selectItems: {
    value: string;
    label: string;
  }[];
  onChange: (newValue: string) => void;
}

function SelectSetting({ value, selectItems, onChange }: SelectSettingProps) {
  let maxWidth = undefined;

  const canvas = document.createElement('canvas');
  const ctx = canvas.getContext('2d');
  if (ctx) {
    ctx.font = '14px "Segoe UI"';

    if (selectItems.every((item) => ctx.measureText(item.label).width <= 350)) {
      maxWidth = '400px';
    }
  }

  return (
    <div style={{ maxWidth }}>
      <SettingSelect
        showSearch
        optionFilterProp="children"
        listHeight={240}
        value={value}
        onChange={(newValue) => onChange(newValue as string)}
      >
        {selectItems.map((item) => (
          <Select.Option key={item.value} value={item.value}>
            {item.label}
          </Select.Option>
        ))}
      </SettingSelect>
    </div>
  );
}

interface NumberSettingProps {
  value?: number;
  onChange: (newValue: number) => void;
}

function NumberSetting({ value, onChange }: NumberSettingProps) {
  return (
    <SettingInputNumber
      value={value}
      min={-2147483648}
      max={2147483647}
      onChange={(newValue) => onChange(parseIntLax(newValue))}
    />
  );
}

interface SettingsTreeProps {
  modSettings: ModSettings;
  onSettingChanged: (key: string, newValue: string | number) => void;
  arrayItemMaxIndex: Record<string, number>;
  onRemoveArrayItem: (key: string, index: number) => void;
  onNewArrayItem: (key: string, index: number) => void;
}

interface SingleSettingProps {
  settingsTreeProps: SettingsTreeProps;
  initialSettingsValue: InitialSettingsValue;
  initialSettingItemExtra?: InitialSettingItemExtra;
  settingKey: string;
}

function SingleSetting({
  settingsTreeProps,
  initialSettingsValue,
  initialSettingItemExtra,
  settingKey,
}: SingleSettingProps) {
  const { modSettings, onSettingChanged } = settingsTreeProps;
  const descriptor = describeSetting(initialSettingsValue);

  switch (descriptor.kind) {
    case SettingType.Boolean:
      return (
        <BooleanSetting
          checked={!!parseIntLax(modSettings[settingKey])}
          onChange={(checked) => onSettingChanged(settingKey, checked ? 1 : 0)}
        />
      );

    case SettingType.Number:
      return (
        <NumberSetting
          value={modSettings[settingKey] === undefined ? undefined : parseIntLax(modSettings[settingKey])}
          onChange={(newValue) => onSettingChanged(settingKey, newValue)}
        />
      );

    case SettingType.String:
      if (initialSettingItemExtra?.options) {
        return (
          <SelectSetting
            value={(modSettings[settingKey] ?? '').toString()}
            selectItems={initialSettingItemExtra.options.map((option) => {
              const [value, label] = Object.entries(option)[0];
              return { value, label };
            })}
            onChange={(newValue) => onSettingChanged(settingKey, newValue)}
          />
        );
      }
      return (
        <StringSetting
          value={(modSettings[settingKey] ?? '').toString()}
          sampleValue={descriptor.value}
          onChange={(newValue) => onSettingChanged(settingKey, newValue)}
        />
      );

    case SettingType.NumberArray:
    case SettingType.StringArray:
    case SettingType.ObjectArray:
      return (
        <ArraySettings
          settingsTreeProps={settingsTreeProps}
          initialSettingsItems={descriptor.value}
          initialSettingItemExtra={initialSettingItemExtra}
          keyPrefix={settingKey}
        />
      );

    case SettingType.NestedObject:
      return (
        <SettingsCard>
          <ObjectSettings
            settingsTreeProps={settingsTreeProps}
            initialSettings={descriptor.value}
            keyPrefix={settingKey + '.'}
          />
        </SettingsCard>
      );
  }
}

interface ArraySettingsProps {
  settingsTreeProps: SettingsTreeProps;
  initialSettingsItems: InitialSettingsArrayValue;
  initialSettingItemExtra?: InitialSettingItemExtra;
  keyPrefix: string;
}

function ArraySettings({
  settingsTreeProps,
  initialSettingsItems,
  initialSettingItemExtra,
  keyPrefix,
}: ArraySettingsProps) {
  const { t } = useTranslation();

  const { modSettings, arrayItemMaxIndex, onRemoveArrayItem, onNewArrayItem } =
    settingsTreeProps;

  const maxSettingsArrayIndex = Object.keys(modSettings).reduce(
    (maxIndex, key) => {
      if (key.startsWith(keyPrefix + '[')) {
        const match = key.slice((keyPrefix + '[').length).match(/^(\d+)\]/);
        if (match) {
          return Math.max(maxIndex, parseIntLax(match[1]));
        }
      }

      return maxIndex;
    },
    -1
  );

  const maxArrayIndex = Math.max(
    maxSettingsArrayIndex,
    arrayItemMaxIndex[keyPrefix] ?? 0
  );

  const indexValues = [...Array(maxArrayIndex + 1).keys(), -1];

  const defaultValue = initialSettingsItems[0];

  return (
    <List
      itemLayout="vertical"
      dataSource={indexValues}
      renderItem={(index) => (
        <SettingsListItem key={index}>
          <div>
            {index === -1 ? (
              <Button
                disabled={maxArrayIndex !== maxSettingsArrayIndex}
                onClick={() => onNewArrayItem(keyPrefix, maxArrayIndex + 1)}
              >
                {t('modDetails.settings.arrayItemAdd')}
              </Button>
            ) : (
              <ArraySettingsItemWrapper>
                <DropdownModal
                  menu={{
                    items: [
                      {
                        label: t('modDetails.settings.arrayItemRemove'),
                        key: 'remove',
                        onClick: () => {
                          dropdownModalDismissed();
                          onRemoveArrayItem(keyPrefix, index)
                        },
                      },
                    ],
                  }}
                  trigger={['click']}
                >
                  <ArraySettingsDropdownOptionsButton>
                    <FontAwesomeIcon icon={faCaretDown} />
                  </ArraySettingsDropdownOptionsButton>
                </DropdownModal>
                <SingleSetting
                  settingsTreeProps={settingsTreeProps}
                  initialSettingsValue={defaultValue}
                  initialSettingItemExtra={initialSettingItemExtra}
                  settingKey={`${keyPrefix}[${index}]`}
                />
              </ArraySettingsItemWrapper>
            )}
          </div>
        </SettingsListItem>
      )}
    />
  );
}

interface ObjectSettingsProps {
  settingsTreeProps: SettingsTreeProps;
  initialSettings: InitialSettings;
  keyPrefix?: string;
}

function ObjectSettings({
  settingsTreeProps,
  initialSettings,
  keyPrefix = '',
}: ObjectSettingsProps) {
  return (
    <List
      itemLayout="vertical"
      split={false}
      dataSource={initialSettings}
      renderItem={(item) => (
        <SettingsListItem key={item.key}>
          <SettingsListItemMeta
            title={item.name || item.key}
            description={item.description}
          />
          <SingleSetting
            settingsTreeProps={settingsTreeProps}
            initialSettingsValue={item.value}
            initialSettingItemExtra={item}
            settingKey={keyPrefix + item.key}
          />
        </SettingsListItem>
      )}
    />
  );
}

interface YamlEditorProps {
  yamlText: string;
  onYamlTextChange: (value: string) => void;
}

function YamlEditor({ yamlText, onYamlTextChange }: YamlEditorProps) {
  const [editorCalcHeight, setEditorCalcHeight] = useState('0');

  return (
    <ConfigProvider direction="ltr">
      <YamlEditorWrapper>
        <Editor
          height={editorCalcHeight}
          defaultLanguage="yaml"
          value={yamlText}
          onChange={(value) => {
            onYamlTextChange(value || '');
          }}
          onMount={(editor, monacoInstance) => {
            // Calculate height based on position
            const rect = editor.getDomNode()?.getBoundingClientRect();
            if (!rect) {
              return;
            }
            const topOffset = rect.top;
            const bottomOffset = 24; // Bottom padding
            const totalOffset = topOffset + bottomOffset;
            setEditorCalcHeight(`calc(100vh - ${totalOffset}px)`);

            // Fix clipboard operations in Electron/webview context Add copy
            // action (Ctrl+C)
            editor.addAction({
              id: 'editor.action.clipboardCopyActionWithExecCommand',
              label: 'Copy',
              keybindings: [monacoInstance.KeyMod.CtrlCmd | monacoInstance.KeyCode.KeyC],
              contextMenuGroupId: '9_cutcopypaste',
              contextMenuOrder: 1,
              run: (ed) => {
                const selection = ed.getSelection();
                const model = ed.getModel();
                if (!selection || !model) return;

                if (selection.isEmpty()) {
                  // No selection - copy the entire current line including newline
                  const lineNumber = selection.startLineNumber;

                  // Select the line including the newline character
                  const lineRange = new monacoInstance.Range(
                    lineNumber, 1,
                    lineNumber + 1, 1
                  );
                  ed.setSelection(lineRange);
                  document.execCommand('copy');
                  // Restore cursor position
                  ed.setSelection(selection);
                } else {
                  // Has selection - copy selected text
                  document.execCommand('copy');
                }
              }
            });

            // Add cut action (Ctrl+X)
            editor.addAction({
              id: 'editor.action.clipboardCutActionWithExecCommand',
              label: 'Cut',
              keybindings: [monacoInstance.KeyMod.CtrlCmd | monacoInstance.KeyCode.KeyX],
              contextMenuGroupId: '9_cutcopypaste',
              contextMenuOrder: 0,
              run: (ed) => {
                const selection = ed.getSelection();
                const model = ed.getModel();
                if (!selection || !model) return;

                if (selection.isEmpty()) {
                  // No selection - cut the entire current line including newline
                  const lineNumber = selection.startLineNumber;

                  // Select the entire line including newline
                  const lineRange = new monacoInstance.Range(
                    lineNumber, 1,
                    lineNumber + 1, 1
                  );
                  ed.setSelection(lineRange);
                  document.execCommand('copy');

                  // Delete the entire line including newline
                  ed.executeEdits('cut', [{
                    range: lineRange,
                    text: '',
                    forceMoveMarkers: true
                  }]);
                } else {
                  // Has selection - cut selected text
                  document.execCommand('copy');
                  ed.executeEdits('cut', [{
                    range: selection,
                    text: '',
                    forceMoveMarkers: true
                  }]);
                }
              }
            });

            // Add paste action (Ctrl+V)
            editor.addAction({
              id: 'editor.action.clipboardPasteActionWithExecCommand',
              label: 'Paste',
              keybindings: [monacoInstance.KeyMod.CtrlCmd | monacoInstance.KeyCode.KeyV],
              contextMenuGroupId: '9_cutcopypaste',
              contextMenuOrder: 2,
              run: async (ed) => {
                try {
                  // Try modern clipboard API first
                  if (navigator.clipboard && navigator.clipboard.readText) {
                    const text = await navigator.clipboard.readText();
                    if (text) {
                      const selection = ed.getSelection();
                      if (selection) {
                        ed.executeEdits('paste', [{
                          range: selection,
                          text: text,
                          forceMoveMarkers: true
                        }]);
                      }
                    }
                  } else {
                    // Fallback to execCommand
                    document.execCommand('paste');
                  }
                } catch (err) {
                  console.error('Paste failed:', err);
                }
              }
            });

            // Add paste action for Shift+Insert
            editor.addAction({
              id: 'editor.action.clipboardPasteActionWithShiftInsert',
              label: 'Paste',
              keybindings: [monacoInstance.KeyMod.Shift | monacoInstance.KeyCode.Insert],
              run: async (ed) => {
                try {
                  if (navigator.clipboard && navigator.clipboard.readText) {
                    const text = await navigator.clipboard.readText();
                    if (text) {
                      const selection = ed.getSelection();
                      if (selection) {
                        ed.executeEdits('paste', [{
                          range: selection,
                          text: text,
                          forceMoveMarkers: true
                        }]);
                      }
                    }
                  } else {
                    document.execCommand('paste');
                  }
                } catch (err) {
                  console.error('Paste failed:', err);
                }
              }
            });

            // Hide the default clipboard actions that don't work in Electron.
            // We need to remove them from the context menu.
            // https://github.com/microsoft/monaco-editor/issues/1280#issuecomment-2099873176
            const removableIds = [
              'editor.action.clipboardCopyAction',
              'editor.action.clipboardCutAction',
              'editor.action.clipboardPasteAction'
            ];
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            const contextmenu = editor.getContribution('editor.contrib.contextmenu') as any;
            if (contextmenu && contextmenu._getMenuActions) {
              const realMethod = contextmenu._getMenuActions;
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
              contextmenu._getMenuActions = function () {
                // eslint-disable-next-line prefer-rest-params
                const items = realMethod.apply(contextmenu, arguments);
                // eslint-disable-next-line @typescript-eslint/no-explicit-any
                return items.filter(function (item: any) {
                  return !removableIds.includes(item.id);
                });
              };
            }
          }}
          options={{
            detectIndentation: false,
            tabSize: 2,
            insertSpaces: true,
            minimap: { enabled: false },
          }}
          theme="vs-dark"
        />
      </YamlEditorWrapper>
    </ConfigProvider>
  );
}

interface Props {
  modId: string;
  initialSettings: InitialSettings;
  onCanNavigateAwayChange?: (canNavigateAway: () => Promise<boolean>) => void;
}

function ModDetailsSettings({ modId, initialSettings, onCanNavigateAwayChange }: Props) {
  const { t } = useTranslation();

  const [modSettingsUI, setModSettingsUI] = useState<ModSettings | null>(mockModSettings);
  const [settingsChanged, setSettingsChanged] = useState(false);
  const [isYamlMode, setIsYamlMode] = useState(() => {
    const stored = localStorage.getItem('settingsYamlMode');
    return stored === 'true';
  });
  const [yamlText, setYamlText] = useState('');
  const [yamlWasEdited, setYamlWasEdited] = useState(false);

  // Track if a confirmation modal is already open
  const isModalOpenRef = useRef(false);

  // Helper function to show confirmation modal for unsaved changes
  const showUnsavedChangesConfirmation = useCallback((): Promise<boolean> => {
    // Prevent multiple modals from opening
    if (isModalOpenRef.current) {
      return Promise.resolve(false);
    }

    isModalOpenRef.current = true;

    return new Promise((resolve) => {
      Modal.confirm({
        title: t('modDetails.settings.unsavedChangesTitle'),
        content: t('modDetails.settings.unsavedChangesMessage'),
        okText: t('modDetails.settings.unsavedChangesLeave'),
        cancelText: t('modDetails.settings.unsavedChangesStay'),
        onOk: () => {
          isModalOpenRef.current = false;
          resolve(true);
        },
        onCancel: () => {
          isModalOpenRef.current = false;
          resolve(false);
        },
        closable: true,
        maskClosable: true,
      });
    });
  }, [t]);

  // Block navigation when there are unsaved changes
  const blocker = useBlocker(({ currentLocation, nextLocation }) => {
    return settingsChanged && currentLocation.pathname !== nextLocation.pathname;
  });

  // Show confirmation modal when navigation is blocked
  useEffect(() => {
    if (blocker.state === 'blocked') {
      showUnsavedChangesConfirmation().then((canLeave) => {
        if (canLeave) {
          blocker.proceed();
        } else {
          blocker.reset();
        }
      });
    }
  }, [blocker, showUnsavedChangesConfirmation]);

  // Provide a callback for parent component to check if navigation is allowed
  useEffect(() => {
    const canNavigateAway = (): Promise<boolean> => {
      if (!settingsChanged) {
        return Promise.resolve(true);
      }

      return showUnsavedChangesConfirmation();
    };

    onCanNavigateAwayChange?.(canNavigateAway);
  }, [settingsChanged, showUnsavedChangesConfirmation, onCanNavigateAwayChange]);

  const { getModSettings } = useGetModSettings(
    useCallback(
      (data) => {
        if (data.modId === modId) {
          setModSettingsUI(data.settings);
        }
      },
      [modId]
    )
  );

  const { setModSettings } = useSetModSettings(
    useCallback(
      (data) => {
        if (data.modId === modId && data.succeeded) {
          setSettingsChanged(false);
        }
      },
      [modId]
    )
  );

  // Initialize YAML validator with schema
  const yamlValidator = useMemo(
    () => new YamlSchemaValidator(initialSettings),
    [initialSettings]
  );

  // YAML conversion handlers
  const settingsToYaml = useCallback(
    (settings: ModSettings): string => YamlConverter.toYaml(settings, initialSettings),
    [initialSettings]
  );

  const yamlToSettings = useCallback(
    (yamlString: string) => YamlConverter.fromYaml(yamlString, yamlValidator, t),
    [yamlValidator, t]
  );

  // Sync YAML text only when switching to YAML mode or on initial load if
  // already in YAML mode. Don't sync when settings change to preserve user's
  // YAML formatting.
  const prevIsYamlMode = useRef<boolean | null>(null);
  useEffect(() => {
    if (!modSettingsUI) {
      return;
    }

    if (isYamlMode && !prevIsYamlMode.current && modSettingsUI) {
      setYamlText(settingsToYaml(modSettingsUI));
    }

    prevIsYamlMode.current = isYamlMode;
  }, [isYamlMode, modSettingsUI, settingsToYaml]);

  // Handle mode toggle
  const handleModeToggle = useCallback(() => {
    if (isYamlMode) {
      // Switching from YAML to UI mode
      if (yamlWasEdited) {
        // YAML was edited - validate and parse it
        const { settings, error } = yamlToSettings(yamlText);
        if (error || !settings) {
          message.error(formatYamlError(error || 'Unknown error'));
          return;
        }
        setModSettingsUI(settings);
      }
      // If YAML was never edited, keep existing modSettingsUI
      setArrayItemMaxIndex({});
      setIsYamlMode(false);
      setYamlText('');
      setYamlWasEdited(false);
      localStorage.setItem('settingsYamlMode', 'false');
    } else {
      // Switching from UI to YAML mode
      setIsYamlMode(true);
      setYamlWasEdited(false);
      localStorage.setItem('settingsYamlMode', 'true');
    }
  }, [isYamlMode, yamlWasEdited, yamlToSettings, yamlText]);

  const handleSave = useCallback(() => {
    if (!settingsChanged) {
      return;
    }

    let settingsToSave = modSettingsUI;

    // If in YAML mode, validate and parse before saving
    if (isYamlMode) {
      const { settings, error } = yamlToSettings(yamlText);
      if (error || !settings) {
        message.error(formatYamlError(error || 'Unknown error'));
        return;
      }
      settingsToSave = settings;
    }

    if (settingsToSave) {
      setModSettings({
        modId,
        settings: settingsToSave,
      });
    }
  }, [settingsChanged, modSettingsUI, isYamlMode, yamlText, yamlToSettings, modId, setModSettings]);

  useEffect(() => {
    getModSettings({ modId });
  }, [getModSettings, modId]);

  useEventListener(
    'keydown',
    useCallback(
      (e: KeyboardEvent) => {
        if (e.key === 's' && e.ctrlKey) {
          e.preventDefault();
          handleSave();
        }
      },
      [handleSave]
    )
  );

  const [arrayItemMaxIndex, setArrayItemMaxIndex] = useState<
    Record<string, number>
  >({});

  const onRemoveArrayItem = useCallback(
    (key: string, index: number) => {
      const indexFromKey = (targetKey: string) => {
        if (targetKey.startsWith(key + '[')) {
          const match = targetKey.slice((key + '[').length).match(/^(\d+)\]/);
          if (match) {
            return parseIntLax(match[1]);
          }
        }
        return null;
      };

      const decreaseKeyIndex = (targetKey: string) => {
        if (targetKey.startsWith(key + '[')) {
          const match = targetKey
            .slice((key + '[').length)
            .match(/^(\d+)(\].*$)/);
          if (match) {
            const targetKeyIndex = parseIntLax(match[1]);
            if (targetKeyIndex > index) {
              return key + '[' + (targetKeyIndex - 1).toString() + match[2];
            }
          }
        }
        return targetKey;
      };

      setModSettingsUI(
        Object.fromEntries(
          Object.entries(modSettingsUI ?? {})
            .filter(([iterKey, iterValue]) => {
              return indexFromKey(iterKey) !== index;
            })
            .map(([iterKey, iterValue]) => {
              return [decreaseKeyIndex(iterKey), iterValue];
            })
        )
      );

      setArrayItemMaxIndex(
        Object.fromEntries(
          Object.entries(arrayItemMaxIndex)
            .filter(([iterKey, iterValue]) => {
              return indexFromKey(iterKey) !== index;
            })
            .map(([iterKey, iterValue]) => {
              return iterKey === key
                ? [iterKey, Math.max(iterValue - 1, 0)]
                : [decreaseKeyIndex(iterKey), iterValue];
            })
        )
      );

      setSettingsChanged(true);
    },
    [modSettingsUI, arrayItemMaxIndex]
  );

  if (modSettingsUI === null) {
    return null;
  }

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        handleSave();
      }}
    >
      <SaveSettingsCard bordered={false} size="small">
        <ActionButtonsWrapper>
          <Button
            type="primary"
            htmlType="submit"
            title="Ctrl+S"
            disabled={!settingsChanged}
          >
            {t('modDetails.settings.saveButton')}
          </Button>
          <Button
            onClick={handleModeToggle}
          >
            {isYamlMode
              ? t('modDetails.settings.uiMode')
              : t('modDetails.settings.yamlMode')
            }
          </Button>
        </ActionButtonsWrapper>
      </SaveSettingsCard>
      {isYamlMode ? (
        <YamlEditor
          yamlText={yamlText}
          onYamlTextChange={(value) => {
            setYamlText(value);
            setSettingsChanged(true);
            setYamlWasEdited(true);
          }}
        />
      ) : (
        <SettingsWrapper>
          <ObjectSettings
            settingsTreeProps={{
              modSettings: modSettingsUI,
              onSettingChanged: (key, newValue) => {
                setModSettingsUI({
                  ...modSettingsUI,
                  [key]: newValue,
                });
                setSettingsChanged(true);
              },
              arrayItemMaxIndex: arrayItemMaxIndex,
              onRemoveArrayItem,
              onNewArrayItem: (key, index) => {
                setArrayItemMaxIndex({
                  ...arrayItemMaxIndex,
                  [key]: index,
                });
                setSettingsChanged(true);
              },
            }}
            initialSettings={initialSettings}
          />
        </SettingsWrapper>
      )}
    </form>
  );
}

export default ModDetailsSettings;

// Types exported for testing only
export type typesForTesting = {
  ModSettings: ModSettings;
  NestedValue: NestedValue;
  NestedSettings: NestedSettings;
  InitialSettings: InitialSettings;
  InitialSettingItem: InitialSettingItem;
  InitialSettingItemExtra: InitialSettingItemExtra;
  InitialSettingsValue: InitialSettingsValue;
  InitialSettingsArrayValue: InitialSettingsArrayValue;
  TypeMismatchError: TypeMismatchError;
}

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
