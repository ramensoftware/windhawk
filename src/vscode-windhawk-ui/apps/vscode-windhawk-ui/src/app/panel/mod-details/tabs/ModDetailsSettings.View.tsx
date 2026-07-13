import { DropdownModal, InputNumberWithContextMenu, InputWithContextMenu, SelectModal } from '@app/components/InputWithContextMenu';
import { showErrorMessage } from '@app/feedback';
import {
  type InitialSettings,
  type InitialSettingsArrayValue,
  type InitialSettingsValue,
} from '@app/webviewIPCMessages';
import { faCaretDown } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Alert, Button, Card, List, Select, Switch } from 'antd';
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import {
  type ModSettings,
  describeSetting,
  parseIntLax,
  SettingType,
  YamlConverter,
  YamlSchemaValidator,
} from './ModSettingsYaml';

// Use webpack constant for conditional compilation
declare const WEBPACK_IS_WEBSITE: boolean;

// Lazy-load Monaco editor only in extension mode
const MonacoYamlEditor = WEBPACK_IS_WEBSITE
  ? null
  : lazy(() => import('./MonacoYamlEditor'));

// ============================================================================
// Styled Components
// ============================================================================

const SettingsWrapper = styled.div`
  // If an object list (with split={false}) is nested inside an array list (without split={false}),
  // the array list's CSS is applied to the object list's CSS, forcing the split style.
  // This CSS rule explicitly removes the split from object lists.
  .ant-list:not(.ant-list-split) > div > div > ul > li.ant-list-item {
    border-bottom: none;
  }

  // Word-wrap long lines.
  overflow-wrap: break-word;

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

const ArraySettingsItemContent = styled.div`
  flex: 1;
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
  flex-wrap: wrap;
  gap: 12px;
`;

const YamlErrorContent = styled.div`
  display: inline-block;
  text-align: start;
  font-family: 'Consolas', 'Monaco', 'Courier New', monospace;
  white-space: break-spaces;
`;

// ============================================================================
// Type Definitions
// ============================================================================

type InitialSettingItemExtra = {
  options?: Record<string, string>[];
};

/**
 * For read-only object arrays: merge metadata (options, name, description) from
 * the schema entry with values from the data entry, so that all array items
 * inherit $options and other annotations from the first entry.
 */
function mergeInitialSettingsMetadata(
  schema: InitialSettings,
  data: InitialSettings
): InitialSettings {
  return schema.map((schemaItem) => {
    const dataItem = data.find((d) => d.key === schemaItem.key);
    return dataItem
      ? { ...schemaItem, value: dataItem.value }
      : schemaItem;
  });
}

// ============================================================================
// Utility Functions
// ============================================================================

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
// Setting Components
// ============================================================================

interface BooleanSettingProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}

function BooleanSetting({ checked, onChange, disabled }: BooleanSettingProps) {
  return <Switch checked={checked} onChange={onChange} disabled={disabled} />;
}

interface StringSettingProps {
  value: string;
  sampleValue: string;
  onChange: (newValue: string) => void;
  readOnly?: boolean;
}

function StringSetting({ value, sampleValue, onChange, readOnly }: StringSettingProps) {
  const { t } = useTranslation();

  let placeholder: string | undefined;
  if (sampleValue) {
    placeholder = t('modDetails.settings.sampleValue') + `: ${sampleValue}`;
  }

  return (
    <InputWithContextMenu
      placeholder={placeholder}
      value={readOnly ? undefined : value}
      onChange={(e) => onChange(e.target.value)}
      readOnly={readOnly}
    />
  );
}

interface SelectSettingProps {
  value: string;
  sampleValue?: string;
  selectItems: {
    value: string;
    label: string;
  }[];
  onChange: (newValue: string) => void;
  readOnly?: boolean;
}

function SelectSetting({ value, sampleValue, selectItems, onChange, readOnly }: SelectSettingProps) {
  let maxWidth = undefined;

  const canvas = document.createElement('canvas');
  const ctx = canvas.getContext('2d');
  if (ctx) {
    ctx.font = '14px "Segoe UI"';

    if (selectItems.every((item) => ctx.measureText(item.label).width <= 350)) {
      maxWidth = '400px';
    }
  }

  let placeholder: string | undefined;
  if (readOnly) {
    placeholder = selectItems.find((item) => item.value === sampleValue)?.label;
  }

  return (
    <div style={{ maxWidth }}>
      <SettingSelect
        showSearch={!readOnly}
        optionFilterProp="children"
        listHeight={240}
        value={readOnly ? undefined : value}
        placeholder={placeholder}
        onChange={(newValue) => {
          if (!readOnly) {
            onChange(newValue as string);
          }
        }}
      >
        {selectItems.map((item) => (
          <Select.Option key={item.value} value={item.value} disabled={readOnly}>
            {item.label}
          </Select.Option>
        ))}
      </SettingSelect>
    </div>
  );
}

interface NumberSettingProps {
  value: number;
  sampleValue?: number;
  onChange: (newValue: number) => void;
  readOnly?: boolean;
}

function NumberSetting({ value, sampleValue, onChange, readOnly }: NumberSettingProps) {
  let placeholder: string | undefined;
  if (readOnly) {
    placeholder = parseIntLax(sampleValue).toString();
  }

  return (
    <SettingInputNumber
      value={readOnly ? undefined : value}
      min={-2147483648}
      max={2147483647}
      onChange={(newValue) => onChange(parseIntLax(newValue))}
      readOnly={readOnly}
      placeholder={placeholder}
    />
  );
}

// ============================================================================
// Settings Tree Components
// ============================================================================

interface SettingsTreeProps {
  modSettings: ModSettings;
  onSettingChanged: (key: string, newValue: string | number) => void;
  arrayItemMaxIndex: Record<string, number>;
  onRemoveArrayItem: (key: string, index: number) => void;
  onNewArrayItem: (key: string, index: number) => void;
  readOnly?: boolean;
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
  const { modSettings, onSettingChanged, readOnly } = settingsTreeProps;
  const descriptor = describeSetting(initialSettingsValue);

  switch (descriptor.kind) {
    case SettingType.Boolean:
      return (
        <BooleanSetting
          checked={readOnly ? descriptor.value : !!parseIntLax(modSettings[settingKey])}
          onChange={(checked) => onSettingChanged(settingKey, checked ? 1 : 0)}
          disabled={readOnly}
        />
      );

    case SettingType.Number:
      return (
        <NumberSetting
          value={parseIntLax(modSettings[settingKey])}
          sampleValue={descriptor.value}
          onChange={(newValue) => onSettingChanged(settingKey, newValue)}
          readOnly={readOnly}
        />
      );

    case SettingType.String:
      if (initialSettingItemExtra?.options) {
        return (
          <SelectSetting
            value={(modSettings[settingKey] ?? '').toString()}
            sampleValue={descriptor.value}
            selectItems={initialSettingItemExtra.options.map((option) => {
              const [value, label] = Object.entries(option)[0];
              return { value, label };
            })}
            onChange={(newValue) => onSettingChanged(settingKey, newValue)}
            readOnly={readOnly}
          />
        );
      }
      return (
        <StringSetting
          value={(modSettings[settingKey] ?? '').toString()}
          sampleValue={descriptor.value}
          onChange={(newValue) => onSettingChanged(settingKey, newValue)}
          readOnly={readOnly}
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

  const { modSettings, arrayItemMaxIndex, onRemoveArrayItem, onNewArrayItem, readOnly } =
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
    arrayItemMaxIndex[keyPrefix] ?? 0,
    readOnly ? initialSettingsItems.length - 1 : -1
  );

  const indexValues = [...Array(maxArrayIndex + 1).keys(), -1];

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
                {!readOnly && (
                  <DropdownModal
                    menu={{
                      items: [
                        {
                          label: t('modDetails.settings.arrayItemRemove'),
                          key: 'remove',
                          onClick: () => {
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
                )}
                <ArraySettingsItemContent>
                  <SingleSetting
                    settingsTreeProps={settingsTreeProps}
                    initialSettingsValue={
                      readOnly
                        ? (Array.isArray(initialSettingsItems[index]) && Array.isArray(initialSettingsItems[0])
                          ? mergeInitialSettingsMetadata(
                            initialSettingsItems[0] as InitialSettings,
                            initialSettingsItems[index] as InitialSettings
                          )
                          : initialSettingsItems[index])
                        : initialSettingsItems[0]
                    }
                    initialSettingItemExtra={initialSettingItemExtra}
                    settingKey={`${keyPrefix}[${index}]`}
                  />
                </ArraySettingsItemContent>
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

// ============================================================================
// Main View Component
// ============================================================================

export interface ModDetailsSettingsViewProps {
  modId: string;
  initialSettings: InitialSettings;

  // Settings state from Extension/Website
  modSettingsUI: ModSettings | null;
  settingsChanged: boolean;

  // Read-only mode (for Website)
  readOnly?: boolean;

  // Callbacks
  onSettingsChange: (newSettings: ModSettings) => void;
  onSave: (settingsToSave: ModSettings) => void;
}

export function ModDetailsSettingsView({
  modId,
  initialSettings,
  modSettingsUI,
  settingsChanged,
  readOnly = false,
  onSettingsChange,
  onSave,
}: ModDetailsSettingsViewProps) {
  const { t } = useTranslation();

  // YAML mode state (managed internally)
  const [isYamlMode, setIsYamlMode] = useState(() => {
    if (readOnly) return false;
    const stored = localStorage.getItem('settingsYamlMode');
    return stored === 'true';
  });
  const [yamlText, setYamlText] = useState('');
  const [yamlWasEdited, setYamlWasEdited] = useState(false);

  // Array item UI state (managed internally)
  const [arrayItemMaxIndex, setArrayItemMaxIndex] = useState<Record<string, number>>({});

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
          showErrorMessage(formatYamlError(error || 'Unknown error'));
          return;
        }
        onSettingsChange(settings);
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
  }, [isYamlMode, yamlWasEdited, yamlToSettings, yamlText, onSettingsChange]);

  // Handle save
  const handleSave = useCallback(() => {
    if (!settingsChanged) {
      return;
    }

    let settingsToSave = modSettingsUI;

    // If in YAML mode, validate and parse before saving
    if (isYamlMode) {
      const { settings, error } = yamlToSettings(yamlText);
      if (error || !settings) {
        showErrorMessage(formatYamlError(error || 'Unknown error'));
        return;
      }
      settingsToSave = settings;
    }

    if (settingsToSave) {
      onSave(settingsToSave);
    }
  }, [settingsChanged, modSettingsUI, isYamlMode, yamlText, yamlToSettings, onSave]);

  // Keyboard shortcut (Ctrl+S)
  useEffect(() => {
    if (readOnly) {
      return;
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 's' && e.ctrlKey) {
        e.preventDefault();
        handleSave();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [handleSave, readOnly]);

  // Handle removing array item
  const onRemoveArrayItem = useCallback(
    (key: string, index: number) => {
      if (!modSettingsUI) return;

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

      const newSettings = Object.fromEntries(
        Object.entries(modSettingsUI)
          .filter(([iterKey]) => {
            return indexFromKey(iterKey) !== index;
          })
          .map(([iterKey, iterValue]) => {
            return [decreaseKeyIndex(iterKey), iterValue];
          })
      );

      onSettingsChange(newSettings);

      setArrayItemMaxIndex(
        Object.fromEntries(
          Object.entries(arrayItemMaxIndex)
            .filter(([iterKey]) => {
              return indexFromKey(iterKey) !== index;
            })
            .map(([iterKey, iterValue]) => {
              return iterKey === key
                ? [iterKey, Math.max(iterValue - 1, 0)]
                : [decreaseKeyIndex(iterKey), iterValue];
            })
        )
      );
    },
    [modSettingsUI, arrayItemMaxIndex, onSettingsChange]
  );

  // Handle setting change (from UI)
  const handleSettingChanged = useCallback(
    (key: string, newValue: string | number) => {
      if (!modSettingsUI) return;
      onSettingsChange({
        ...modSettingsUI,
        [key]: newValue,
      });
    },
    [modSettingsUI, onSettingsChange]
  );

  // Handle new array item
  const handleNewArrayItem = useCallback(
    (key: string, index: number) => {
      setArrayItemMaxIndex({
        ...arrayItemMaxIndex,
        [key]: index,
      });
      // Notify parent that settings changed (even though we just added an empty slot)
      if (modSettingsUI) {
        onSettingsChange({ ...modSettingsUI });
      }
    },
    [arrayItemMaxIndex, modSettingsUI, onSettingsChange]
  );

  // Handle YAML text change
  const handleYamlTextChange = useCallback(
    (value: string) => {
      setYamlText(value);
      setYamlWasEdited(true);
      // Notify parent that settings changed
      if (modSettingsUI) {
        onSettingsChange({ ...modSettingsUI });
      }
    },
    [modSettingsUI, onSettingsChange]
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
        {readOnly && (
          <Alert
            type="info"
            message={t('modDetails.settings.readOnlyPreview')}
          />
        )}
        {!readOnly && (
          <ActionButtonsWrapper>
            <Button
              type="primary"
              htmlType="submit"
              title="Ctrl+S"
              disabled={!settingsChanged}
            >
              {t('modDetails.settings.saveButton')}
            </Button>
            {MonacoYamlEditor && (
              <Button onClick={handleModeToggle}>
                {isYamlMode
                  ? t('modDetails.settings.uiMode')
                  : t('modDetails.settings.yamlMode')
                }
              </Button>
            )}
          </ActionButtonsWrapper>
        )}
      </SaveSettingsCard>
      {isYamlMode && MonacoYamlEditor ? (
        <Suspense fallback={null}>
          <MonacoYamlEditor
            yamlText={yamlText}
            onYamlTextChange={handleYamlTextChange}
          />
        </Suspense>
      ) : (
        <SettingsWrapper>
          <ObjectSettings
            settingsTreeProps={{
              modSettings: modSettingsUI,
              onSettingChanged: handleSettingChanged,
              arrayItemMaxIndex: arrayItemMaxIndex,
              onRemoveArrayItem,
              onNewArrayItem: handleNewArrayItem,
              readOnly,
            }}
            initialSettings={initialSettings}
          />
        </SettingsWrapper>
      )}
    </form>
  );
}
