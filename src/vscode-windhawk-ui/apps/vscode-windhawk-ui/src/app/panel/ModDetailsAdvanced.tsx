import { Alert, Button, Dropdown, List, message, Select, Space, Switch } from 'antd';
import { useCallback, useEffect, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { SelectModal, TextAreaWithContextMenu } from '../components/InputWithContextMenu';
import {
  showAdvancedDebugLogOutput,
  useGetModConfig,
  useGetModSettings,
  useSetModSettings,
  useUpdateModConfig,
} from '../webviewIPC';

const SettingsListItemMeta = styled(List.Item.Meta)`
  .ant-list-item-meta {
    margin-bottom: 8px;
  }

  .ant-list-item-meta-title {
    margin-bottom: 0;
  }
`;

const SettingsSelect = styled(SelectModal)`
  width: 200px;
`;

const SpaceWithWidth = styled(Space)`
  width: 100%;
  max-width: 600px;
`;

function engineArrayToProcessList(processArray: string[]) {
  return processArray.join('\n');
}

function engineProcessListToArray(processList: string) {
  return processList
    .split('\n')
    .map((x) => x.replace(/["/<>|]/g, '').trim())
    .filter((x) => x);
}

interface Props {
  modId: string;
}

function ModDetailsAdvanced({ modId }: Props) {
  const { t } = useTranslation();

  const [debugLogging, setDebugLogging] = useState<number>();
  const [modSettingsUI, setModSettingsUI] = useState<string>();
  const [modSettingsUIModified, setModSettingsUIModified] = useState(false);
  const [customInclude, setCustomInclude] = useState<string>();
  const [customIncludeModified, setCustomIncludeModified] = useState(false);
  const [customExclude, setCustomExclude] = useState<string>();
  const [customExcludeModified, setCustomExcludeModified] = useState(false);
  const [includeExcludeCustomOnly, setIncludeExcludeCustomOnly] =
    useState<boolean>();
  const [patternsMatchCriticalSystemProcesses, setPatternsMatchCriticalSystemProcesses] =
    useState<boolean>();

  const { getModConfig } = useGetModConfig(
    useCallback((data) => {
      if (data.config?.debugLoggingEnabled) {
        setDebugLogging(2);
      } else if (data.config?.loggingEnabled) {
        setDebugLogging(1);
      } else {
        setDebugLogging(0);
      }

      setCustomInclude(engineArrayToProcessList(data.config?.includeCustom ?? []));
      setCustomExclude(engineArrayToProcessList(data.config?.excludeCustom ?? []));
      setIncludeExcludeCustomOnly(
        data.config?.includeExcludeCustomOnly ?? false
      );
      setPatternsMatchCriticalSystemProcesses(
        data.config?.patternsMatchCriticalSystemProcesses ?? false
      );
    }, [])
  );

  const { getModSettings } = useGetModSettings<{ formatted?: boolean }>(
    useCallback((data, context) => {
      setModSettingsUI(
        JSON.stringify(data.settings, null, context?.formatted ? 2 : undefined)
      );
    }, [])
  );

  const { setModSettings } = useSetModSettings(
    useCallback((data) => {
      if (data.succeeded) {
        setModSettingsUIModified(false);
      }
    }, [])
  );

  const { updateModConfig } = useUpdateModConfig<{ callback?: () => void }>(
    useCallback((data, context) => {
      if (data.succeeded) {
        context?.callback?.();
      }
    }, [])
  );

  useEffect(() => {
    getModConfig({ modId });
    getModSettings({ modId });
  }, [getModConfig, getModSettings, modId]);

  if (
    modSettingsUI === undefined ||
    debugLogging === undefined ||
    customInclude === undefined ||
    customExclude === undefined ||
    includeExcludeCustomOnly === undefined ||
    patternsMatchCriticalSystemProcesses === undefined
  ) {
    return null;
  }

  return (
    <List itemLayout="vertical" split={false}>
      <List.Item>
        <SettingsListItemMeta
          title={t('modDetails.advanced.debugLogging.title')}
          description={t('modDetails.advanced.debugLogging.description')}
        />
        <Space direction="vertical" size="middle">
          <SettingsSelect
            value={debugLogging}
            onChange={(value) => {
              const numValue = typeof value === 'number' ? value : 0;
              setDebugLogging(numValue);
              updateModConfig({
                modId,
                config: {
                  loggingEnabled: numValue === 1,
                  debugLoggingEnabled: numValue === 2,
                },
              });
            }}
            dropdownMatchSelectWidth={false}
          >
            <Select.Option key="none" value={0}>
              {t('modDetails.advanced.debugLogging.none')}
            </Select.Option>
            <Select.Option key="error" value={1}>
              {t('modDetails.advanced.debugLogging.modLogs')}
            </Select.Option>
            <Select.Option key="verbose" value={2}>
              {t('modDetails.advanced.debugLogging.detailedLogs')}
            </Select.Option>
          </SettingsSelect>
          <Button
            type="primary"
            onClick={() => {
              showAdvancedDebugLogOutput();
            }}
          >
            {t('modDetails.advanced.debugLogging.showLogButton')}
          </Button>
        </Space>
      </List.Item>
      <List.Item>
        <SettingsListItemMeta
          title={t('modDetails.advanced.modSettings.title')}
          description={t('modDetails.advanced.modSettings.description')}
        />
        <SpaceWithWidth direction="vertical" size="middle">
          <TextAreaWithContextMenu
            rows={4}
            value={modSettingsUI}
            onChange={(e) => {
              setModSettingsUI(e.target.value);
              setModSettingsUIModified(true);
            }}
          />
          <Space>
            <Dropdown.Button
              type="primary"
              menu={{
                items: [
                  {
                    key: 'formatted',
                    label: t(
                      'modDetails.advanced.modSettings.loadFormattedButton'
                    ),
                  },
                ],
                onClick: (e) => {
                  getModSettings(
                    { modId },
                    { formatted: e.key === 'formatted' }
                  );
                },
              }}
              onClick={() => {
                getModSettings({ modId });
              }}
            >
              {t('modDetails.advanced.modSettings.loadButton')}
            </Dropdown.Button>
            <Button
              type="primary"
              disabled={!modSettingsUIModified}
              onClick={() => {
                let settings = null;
                try {
                  settings = JSON.parse(modSettingsUI);
                } catch {
                  message.error(
                    t('modDetails.advanced.modSettings.invalidData')
                  );
                  return;
                }
                setModSettings({
                  modId,
                  settings,
                });
              }}
            >
              {t('modDetails.advanced.modSettings.saveButton')}
            </Button>
          </Space>
        </SpaceWithWidth>
      </List.Item>
      <List.Item>
        <SettingsListItemMeta
          title={t('modDetails.advanced.customList.titleInclusion')}
          description={t(
            'modDetails.advanced.customList.descriptionInclusion'
          )}
        />
        <SpaceWithWidth direction="vertical" size="middle">
          <div>
            <TextAreaWithContextMenu
              rows={4}
              value={customInclude}
              placeholder={
                (t(
                  'modDetails.advanced.customList.processListPlaceholder'
                ) as string) +
                '\n' +
                'notepad.exe\n' +
                '%ProgramFiles%\\Notepad++\\notepad++.exe\n' +
                'C:\\Windows\\system32\\*'
              }
              onChange={(e) => {
                setCustomInclude(e.target.value);
                setCustomIncludeModified(true);
              }}
            />
            {customInclude.match(/["/<>|]/) && (
              <Alert
                description={t('modDetails.advanced.customList.invalidCharactersWarning', {
                  invalidCharacters: '" / < > |',
                })}
                type="warning"
                showIcon
              />
            )}
          </div>
          <Button
            type="primary"
            disabled={!customIncludeModified}
            onClick={() => {
              updateModConfig(
                {
                  modId,
                  config: {
                    includeCustom: engineProcessListToArray(customInclude),
                  },
                },
                { callback: () => setCustomIncludeModified(false) }
              );
            }}
          >
            {t('modDetails.advanced.customList.saveButton')}
          </Button>
        </SpaceWithWidth>
      </List.Item>
      <List.Item>
        <SettingsListItemMeta
          title={t('modDetails.advanced.customList.titleExclusion')}
          description={t(
            'modDetails.advanced.customList.descriptionExclusion'
          )}
        />
        <SpaceWithWidth direction="vertical" size="middle">
          <div>
            <TextAreaWithContextMenu
              rows={4}
              value={customExclude}
              placeholder={
                (t(
                  'modDetails.advanced.customList.processListPlaceholder'
                ) as string) +
                '\n' +
                'notepad.exe\n' +
                '%ProgramFiles%\\Notepad++\\notepad++.exe\n' +
                'C:\\Windows\\system32\\*'
              }
              onChange={(e) => {
                setCustomExclude(e.target.value);
                setCustomExcludeModified(true);
              }}
            />
            {customExclude.match(/["/<>|]/) && (
              <Alert
                description={t('modDetails.advanced.customList.invalidCharactersWarning', {
                  invalidCharacters: '" / < > |',
                })}
                type="warning"
                showIcon
              />
            )}
          </div>
          <Button
            type="primary"
            disabled={!customExcludeModified}
            onClick={() => {
              updateModConfig(
                {
                  modId,
                  config: {
                    excludeCustom: engineProcessListToArray(customExclude),
                  },
                },
                { callback: () => setCustomExcludeModified(false) }
              );
            }}
          >
            {t('modDetails.advanced.customList.saveButton')}
          </Button>
        </SpaceWithWidth>
      </List.Item>
      <List.Item>
        <SettingsListItemMeta
          title={t('modDetails.advanced.includeExcludeCustomOnly.title')}
          description={t(
            'modDetails.advanced.includeExcludeCustomOnly.description'
          )}
        />
        <Switch
          checked={includeExcludeCustomOnly}
          onChange={(checked) => {
            setIncludeExcludeCustomOnly(checked);
            updateModConfig({
              modId,
              config: {
                includeExcludeCustomOnly: checked,
              },
            });
          }}
        />
      </List.Item>
      <List.Item>
        <SettingsListItemMeta
          title={t('modDetails.advanced.patternsMatchCriticalSystemProcesses.title')}
          description={
            <Trans
              t={t}
              i18nKey="modDetails.advanced.patternsMatchCriticalSystemProcesses.description"
              components={[
                <code />,
                <a href="https://github.com/ramensoftware/windhawk/wiki/Injection-targets-and-critical-system-processes">wiki</a>,
              ]}
            />
          }
        />
        <Switch
          checked={patternsMatchCriticalSystemProcesses}
          onChange={(checked) => {
            setPatternsMatchCriticalSystemProcesses(checked);
            updateModConfig({
              modId,
              config: {
                patternsMatchCriticalSystemProcesses: checked,
              },
            });
          }}
        />
      </List.Item>
    </List>
  );
}

export default ModDetailsAdvanced;
