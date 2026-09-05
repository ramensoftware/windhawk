import { Alert, Button, Dropdown, List, Select, Space, Switch } from 'antd';
import { type KeyboardEvent, useCallback, useEffect, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { SelectModal, TextAreaWithContextMenu } from '@app/components/InputWithContextMenu';
import { showErrorMessage } from '@app/feedback';
import {
  showAdvancedDebugLogOutput,
  useGetModConfig,
  useGetModSettings,
  useSetModSettings,
  useUpdateModConfig,
} from '@app/webviewIPC';

// antd draws the meta on the node this styles rather than inside one, so the gap
// under it is set here and not reached for as a descendant. Doubled to carry the
// weight of the .ant-list-vertical .ant-list-item-meta it has to beat.
const SettingsListItemMeta = styled(List.Item.Meta)`
  && {
    margin-bottom: 8px;
  }

  && .ant-list-item-meta-title {
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

function isModSettings(
  value: unknown
): value is Record<string, string | number> {
  return (
    typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value) &&
    Object.values(value).every(
      (v) => typeof v === 'string' || typeof v === 'number'
    )
  );
}

function handleSaveShortcut(
  e: KeyboardEvent<HTMLTextAreaElement>,
  save: () => void
) {
  if (e.key === 's' && e.ctrlKey) {
    e.preventDefault();
    save();
  }
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

  const { getModConfig } = useGetModConfig();
  const { getModSettings } = useGetModSettings();
  const { setModSettings } = useSetModSettings();
  const { updateModConfig } = useUpdateModConfig();

  const loadModConfig = useCallback(async () => {
    const result = await getModConfig({ modId });
    if (result.status !== 'reply') {
      return;
    }

    const config = result.data.config;
    if (config?.debugLoggingEnabled) {
      setDebugLogging(2);
    } else if (config?.loggingEnabled) {
      setDebugLogging(1);
    } else {
      setDebugLogging(0);
    }

    setCustomInclude(engineArrayToProcessList(config?.includeCustom ?? []));
    setCustomExclude(engineArrayToProcessList(config?.excludeCustom ?? []));
    setIncludeExcludeCustomOnly(config?.includeExcludeCustomOnly ?? false);
    setPatternsMatchCriticalSystemProcesses(
      config?.patternsMatchCriticalSystemProcesses ?? false
    );
  }, [getModConfig, modId]);

  // Reads the settings into the text area, formatted or as compact as the host
  // sends them - which is what the load button's two entries choose between.
  const loadModSettings = useCallback(
    async (formatted?: boolean) => {
      const result = await getModSettings({ modId });
      if (result.status !== 'reply') {
        return;
      }

      setModSettingsUI(
        JSON.stringify(result.data.settings, null, formatted ? 2 : undefined)
      );
    },
    [getModSettings, modId]
  );

  useEffect(() => {
    void (async () => {
      // Two round trips of their own, so neither waits on the other.
      await Promise.all([loadModConfig(), loadModSettings()]);
    })();
  }, [loadModConfig, loadModSettings]);

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

  // Every write below takes effect on screen only once its own reply confirms
  // it, so a control the host refused keeps reading as what the host still holds.
  const saveModSettings = async () => {
    if (!modSettingsUIModified) {
      return;
    }
    const trimmed = modSettingsUI.trim();
    let settings: unknown = {};
    if (trimmed !== '') {
      try {
        settings = JSON.parse(trimmed);
      } catch {
        settings = undefined;
      }
    }
    if (!isModSettings(settings)) {
      showErrorMessage(t('modDetails.advanced.modSettings.invalidData'));
      return;
    }
    const result = await setModSettings({ modId, settings });
    if (result.status === 'reply' && result.data.succeeded) {
      setModSettingsUIModified(false);
    }
  };

  const saveDebugLogging = async (level: number) => {
    const result = await updateModConfig({
      modId,
      config: {
        loggingEnabled: level === 1,
        debugLoggingEnabled: level === 2,
      },
    });
    if (result.status === 'reply' && result.data.succeeded) {
      setDebugLogging(level);
    }
  };

  const saveCustomInclude = async () => {
    if (!customIncludeModified) {
      return;
    }
    const result = await updateModConfig({
      modId,
      config: {
        includeCustom: engineProcessListToArray(customInclude),
      },
    });
    if (result.status === 'reply' && result.data.succeeded) {
      setCustomIncludeModified(false);
    }
  };

  const saveCustomExclude = async () => {
    if (!customExcludeModified) {
      return;
    }
    const result = await updateModConfig({
      modId,
      config: {
        excludeCustom: engineProcessListToArray(customExclude),
      },
    });
    if (result.status === 'reply' && result.data.succeeded) {
      setCustomExcludeModified(false);
    }
  };

  const saveIncludeExcludeCustomOnly = async (checked: boolean) => {
    const result = await updateModConfig({
      modId,
      config: {
        includeExcludeCustomOnly: checked,
      },
    });
    if (result.status === 'reply' && result.data.succeeded) {
      setIncludeExcludeCustomOnly(checked);
    }
  };

  const savePatternsMatchCriticalSystemProcesses = async (checked: boolean) => {
    const result = await updateModConfig({
      modId,
      config: {
        patternsMatchCriticalSystemProcesses: checked,
      },
    });
    if (result.status === 'reply' && result.data.succeeded) {
      setPatternsMatchCriticalSystemProcesses(checked);
    }
  };

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
              void saveDebugLogging(typeof value === 'number' ? value : 0);
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
            onKeyDown={(e) => handleSaveShortcut(e, saveModSettings)}
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
                  void loadModSettings(e.key === 'formatted');
                },
              }}
              onClick={() => {
                void loadModSettings();
              }}
            >
              {t('modDetails.advanced.modSettings.loadButton')}
            </Dropdown.Button>
            <Button
              type="primary"
              disabled={!modSettingsUIModified}
              onClick={saveModSettings}
            >
              {t('general.actions.save')}
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
              onKeyDown={(e) => handleSaveShortcut(e, saveCustomInclude)}
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
            onClick={saveCustomInclude}
          >
            {t('general.actions.save')}
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
              onKeyDown={(e) => handleSaveShortcut(e, saveCustomExclude)}
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
            onClick={saveCustomExclude}
          >
            {t('general.actions.save')}
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
            void saveIncludeExcludeCustomOnly(checked);
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
            void savePatternsMatchCriticalSystemProcesses(checked);
          }}
        />
      </List.Item>
    </List>
  );
}

export default ModDetailsAdvanced;
