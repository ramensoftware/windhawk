import { faCaretDown } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, Card, List, Select, Switch } from 'antd';
import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { DropdownModal, dropdownModalDismissed, InputNumberWithContextMenu, InputWithContextMenu, SelectModal } from '../components/InputWithContextMenu';
import { useGetModSettings, useSetModSettings } from '../webviewIPC';
import { mockModSettings } from './mockData';

const SettingsWrapper = styled.div`
  // If an object list (with split={false}) is nested inside an array list (without split={false}),
  // the array list's CSS is applied to the object list's CSS, forcing the split style.
  // This CSS rule explicitly removes the split from object lists.
  .ant-list:not(.ant-list-split) > div > div > ul > li.ant-list-item {
    border-bottom: none;
  }
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
  padding-left: 10px;
  padding-right: 10px;
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
  margin-left: -12px;
  margin-right: -12px;
  margin-top: -12px;
`;

type ModSettings = Record<string, string | number>;

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

function parseIntLax(value?: string | number | null) {
  const result = parseInt((value ?? 0).toString(), 10);
  return Number.isNaN(result) ? 0 : result;
}

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

  if (typeof initialSettingsValue === 'boolean') {
    return (
      <BooleanSetting
        checked={!!parseIntLax(modSettings[settingKey])}
        onChange={(checked) => onSettingChanged(settingKey, checked ? 1 : 0)}
      />
    );
  }

  if (typeof initialSettingsValue === 'number') {
    return (
      <NumberSetting
        value={
          modSettings[settingKey] === undefined
            ? undefined
            : parseIntLax(modSettings[settingKey])
        }
        onChange={(newValue) => onSettingChanged(settingKey, newValue)}
      />
    );
  }

  if (typeof initialSettingsValue === 'string') {
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
    } else {
      return (
        <StringSetting
          value={(modSettings[settingKey] ?? '').toString()}
          sampleValue={initialSettingsValue}
          onChange={(newValue) => onSettingChanged(settingKey, newValue)}
        />
      );
    }
  }

  if (
    Array.isArray(initialSettingsValue) &&
    (typeof initialSettingsValue[0] === 'number' ||
      typeof initialSettingsValue[0] === 'string' ||
      Array.isArray(initialSettingsValue[0]))
  ) {
    return (
      <SettingsCard>
        <ArraySettings
          settingsTreeProps={settingsTreeProps}
          initialSettingsItems={
            initialSettingsValue as InitialSettingsArrayValue
          }
          initialSettingItemExtra={initialSettingItemExtra}
          keyPrefix={settingKey}
        />
      </SettingsCard>
    );
  }

  if (
    Array.isArray(initialSettingsValue) &&
    !Array.isArray(initialSettingsValue[0])
  ) {
    return (
      <SettingsCard>
        <ObjectSettings
          settingsTreeProps={settingsTreeProps}
          initialSettings={initialSettingsValue as InitialSettings}
          keyPrefix={settingKey + '.'}
        />
      </SettingsCard>
    );
  }

  return <div>Internal error, please report the problem</div>;
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
        <List.Item key={index}>
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
        </List.Item>
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
        <List.Item key={item.key}>
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
        </List.Item>
      )}
    />
  );
}

interface Props {
  modId: string;
  initialSettings: InitialSettings;
}

function ModDetailsSettings({ modId, initialSettings }: Props) {
  const { t } = useTranslation();

  const [modSettingsUI, setModSettingsUI] = useState<ModSettings | null>(mockModSettings);

  const [settingsChanged, setSettingsChanged] = useState(false);

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

  useEffect(() => {
    getModSettings({ modId });
  }, [getModSettings, modId]);

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
    <>
      <SaveSettingsCard bordered={false} size="small">
        <Button
          type="primary"
          disabled={!settingsChanged}
          onClick={() => {
            setModSettings({
              modId,
              settings: modSettingsUI,
            });
          }}
        >
          {t('modDetails.settings.saveButton')}
        </Button>
      </SaveSettingsCard>
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
    </>
  );
}

export default ModDetailsSettings;
