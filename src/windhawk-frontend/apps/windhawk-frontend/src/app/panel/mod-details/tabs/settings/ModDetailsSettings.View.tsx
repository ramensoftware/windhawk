import { DropdownModal, InputNumberWithContextMenu, InputWithContextMenu, SelectModal } from '@app/components/InputWithContextMenu';
import {
  type InitialSettings,
  type InitialSettingsArrayValue,
  type InitialSettingsValue,
} from '@app/webviewIPCMessages';
import { faCaretDown, faCompress, faExpand } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Alert, Button, Card, List, Select, Switch } from 'antd';
import { lazy, Suspense, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled, { css } from 'styled-components';
import { type ModSettings, describeSetting, parseIntLax, SettingType } from './core/yamlConverter';
import { materializedMaxIndex } from './core/editorState';
import { type EditorViewModel } from './useModSettingsEditor';

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

// Fullscreen turns the form into a fixed overlay that scrolls as a whole, with
// the action bar pinned by the same position: sticky it already uses inside the
// panel - so the inner layout is shared between the two modes. The side/bottom
// inset mirrors the outer card body padding; the top is left to the sticky
// toolbar, which pins flush to the top edge.
const SettingsForm = styled.form<{ $fullscreen: boolean }>`
  ${({ $fullscreen }) =>
    $fullscreen &&
    css`
      position: fixed;
      inset: 0;
      z-index: 100;
      overflow-y: auto;
      padding: 0 24px 24px;
      background-color: var(--whui-card-background-color);
    `}
`;

const SaveSettingsCard = styled(Card) <{ $fullscreen: boolean }>`
  position: sticky;
  top: 0;
  z-index: 1;
  margin-inline-start: -12px;
  margin-inline-end: -12px;
  margin-top: -12px;

  ${({ $fullscreen }) =>
    $fullscreen &&
    css`
      margin-top: 0;
      padding-top: 12px;
    `}
`;

const SaveSettingsHeader = styled.div`
  display: flex;
  align-items: center;
  gap: 12px;
`;

const SaveSettingsHeaderMain = styled.div`
  flex: 1;
  min-width: 0;
`;

const FullscreenButton = styled(Button)`
  flex-shrink: 0;
`;

const ActionButtonsWrapper = styled.div`
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
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

  const maxSettingsArrayIndex = materializedMaxIndex(modSettings, keyPrefix);

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
        <SettingsListItem
          key={item.key}
          data-testid="mod-setting"
          data-setting-key={keyPrefix + item.key}
        >
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

export interface ModDetailsSettingsViewProps extends EditorViewModel {
  initialSettings: InitialSettings;

  // Read-only mode (for Website and the extension's preview views).
  readOnly?: boolean;
}

export function ModDetailsSettingsView({
  initialSettings,
  readOnly = false,
  mode,
  draft,
  arrayMaxIndex,
  yamlText,
  isDirty,
  yamlAvailable,
  onChangeSetting,
  onAddArrayItem,
  onRemoveArrayItem,
  onSetYamlText,
  onToggleMode,
  onSave,
}: ModDetailsSettingsViewProps) {
  const { t } = useTranslation();

  // Fullscreen state: expand the settings to fill the whole window.
  const [isFullscreen, setIsFullscreen] = useState(false);

  // Mark the body while fullscreen so app-level fixed overlays (e.g. the
  // "Create a new mod" button) can hide themselves behind the settings.
  useEffect(() => {
    if (readOnly) {
      return;
    }

    const className = 'windhawk-settings-fullscreen';
    document.body.classList.toggle(className, isFullscreen);
    return () => document.body.classList.remove(className);
  }, [isFullscreen, readOnly]);

  // Keyboard shortcut (F11) to toggle fullscreen. Not available in preview mode.
  useEffect(() => {
    if (readOnly) {
      return;
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'F11') {
        e.preventDefault();
        setIsFullscreen((value) => !value);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [readOnly]);

  // Keyboard shortcut (Ctrl+S) to save.
  useEffect(() => {
    if (readOnly) {
      return;
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 's' && e.ctrlKey) {
        e.preventDefault();
        onSave();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [onSave, readOnly]);

  const showYamlEditor = mode === 'yaml' && !!MonacoYamlEditor;

  const fullscreenLabel = isFullscreen
    ? t('modDetails.settings.collapse')
    : t('modDetails.settings.expand');

  return (
    <SettingsForm
      $fullscreen={isFullscreen}
      onSubmit={(e) => {
        e.preventDefault();
        onSave();
      }}
    >
      <SaveSettingsCard $fullscreen={isFullscreen} bordered={false} size="small">
        <SaveSettingsHeader>
          <SaveSettingsHeaderMain>
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
                  disabled={!isDirty}
                  data-testid="mod-settings-save"
                >
                  {t('modDetails.settings.saveButton')}
                </Button>
                {MonacoYamlEditor && yamlAvailable && (
                  <Button
                    data-testid="mod-settings-mode-toggle"
                    onClick={onToggleMode}
                  >
                    {mode === 'yaml'
                      ? t('modDetails.settings.uiMode')
                      : t('modDetails.settings.yamlMode')
                    }
                  </Button>
                )}
              </ActionButtonsWrapper>
            )}
          </SaveSettingsHeaderMain>
          {!readOnly && (
            <FullscreenButton
              title={`${fullscreenLabel} (F11)`}
              aria-label={fullscreenLabel}
              onClick={() => setIsFullscreen((value) => !value)}
            >
              <FontAwesomeIcon icon={isFullscreen ? faCompress : faExpand} />
            </FullscreenButton>
          )}
        </SaveSettingsHeader>
      </SaveSettingsCard>
      {showYamlEditor ? (
        <Suspense fallback={null}>
          <MonacoYamlEditor
            yamlText={yamlText}
            onYamlTextChange={onSetYamlText}
            fullscreen={isFullscreen}
          />
        </Suspense>
      ) : (
        <SettingsWrapper>
          <ObjectSettings
            settingsTreeProps={{
              modSettings: draft,
              onSettingChanged: onChangeSetting,
              arrayItemMaxIndex: arrayMaxIndex,
              onRemoveArrayItem,
              onNewArrayItem: onAddArrayItem,
              readOnly,
            }}
            initialSettings={initialSettings}
          />
        </SettingsWrapper>
      )}
    </SettingsForm>
  );
}
