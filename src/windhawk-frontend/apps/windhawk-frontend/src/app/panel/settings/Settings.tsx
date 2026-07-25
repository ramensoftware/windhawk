import { AppUISettingsContext } from '@app/appUISettings';
import { InputNumberWithContextMenu, SelectModal, TextAreaWithContextMenu } from '@app/components/InputWithContextMenu';
import { appLanguages } from '@app/constants/languages';
import { useTheme } from '@app/theme';
import { sanitizeUrl, testIdProps } from '@app/utils';
import { useGetAppSettings, useUpdateAppSettings } from '@app/webviewIPC';
import { type AppSettings } from '@app/webviewIPCMessages';
import { Alert, Badge, Button, Checkbox, Collapse, List, Modal, Segmented, Select, Space, Switch, Tooltip } from 'antd';
import { useCallback, useContext, useEffect, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';

import { UserDataSection } from './userData';

const SettingsWrapper = styled.div`
  padding-bottom: 20px;
`;

const SettingsList = styled(List)`
  margin-bottom: 20px;
`;

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

const SettingsNotice = styled.div`
  margin-top: 14px;
  color: var(--whui-text-muted);
`;

const SettingInputNumber = styled(InputNumberWithContextMenu)`
  width: 100%;
  max-width: 130px;

  // Remove default VSCode focus highlighting color.
  input:focus {
    outline: none !important;
  }
`;

function parseIntLax(value?: string | number | null) {
  const result = parseInt((value ?? 0).toString(), 10);
  return Number.isNaN(result) ? 0 : result;
}

function engineArrayToProcessList(processArray: string[]) {
  return processArray.join('\n');
}

function engineProcessListToArray(processList: string) {
  return processList
    .split('\n')
    .map((x) => x.replace(/["/<>|]/g, '').trim())
    .filter((x) => x);
}

function Settings() {
  const { t, i18n } = useTranslation();
  const appLanguage = i18n.resolvedLanguage;

  const { loggingEnabled } = useContext(AppUISettingsContext);

  const { theme, setTheme } = useTheme();

  const [appSettings, setAppSettings] = useState<Partial<AppSettings> | null>(
    null
  );

  // More advanced settings.
  const [appLoggingVerbosity, setAppLoggingVerbosity] = useState(0);
  const [engineLoggingVerbosity, setEngineLoggingVerbosity] = useState(0);
  const [engineInclude, setEngineInclude] = useState('');
  const [engineExclude, setEngineExclude] = useState('');
  const [engineInjectIntoCriticalProcesses, setEngineInjectIntoCriticalProcesses] = useState(false);
  const [engineInjectIntoIncompatiblePrograms, setEngineInjectIntoIncompatiblePrograms] = useState(false);
  const [engineInjectIntoGames, setEngineInjectIntoGames] = useState(false);

  const resetMoreAdvancedSettings = useCallback(() => {
    setAppLoggingVerbosity(appSettings?.loggingVerbosity ?? 0);
    setEngineLoggingVerbosity(appSettings?.engine?.loggingVerbosity ?? 0);
    setEngineInclude(engineArrayToProcessList(appSettings?.engine?.include ?? []));
    setEngineExclude(engineArrayToProcessList(appSettings?.engine?.exclude ?? []));
    setEngineInjectIntoCriticalProcesses(appSettings?.engine?.injectIntoCriticalProcesses ?? false);
    setEngineInjectIntoIncompatiblePrograms(appSettings?.engine?.injectIntoIncompatiblePrograms ?? false);
    setEngineInjectIntoGames(appSettings?.engine?.injectIntoGames ?? false);
  }, [appSettings]);

  const { getAppSettings } = useGetAppSettings(
    useCallback((data) => {
      setAppSettings(data.appSettings);
    }, [])
  );

  useEffect(() => {
    getAppSettings({});
  }, [getAppSettings]);

  // An import can overwrite the app settings on disk; re-read them so the options below
  // reflect the imported values without needing to leave and re-enter this page. Called
  // both mid-import (the app settings are applied before the mods) and at its end.
  const refreshAppSettings = useCallback(() => {
    getAppSettings({});
  }, [getAppSettings]);

  const { updateAppSettings } = useUpdateAppSettings(
    useCallback(
      (data) => {
        if (data.succeeded && appSettings) {
          setAppSettings({
            ...appSettings,
            ...data.appSettings,
          });
        }
      },
      [appSettings]
    )
  );

  const [isMoreAdvancedSettingsModalOpen, setIsMoreAdvancedSettingsModalOpen] =
    useState(false);

  if (!appSettings) {
    return null;
  }

  const includeListEmpty = engineInclude.trim() === '';
  const excludeListEmpty = engineExclude.trim() === '' &&
    engineInjectIntoCriticalProcesses &&
    engineInjectIntoIncompatiblePrograms &&
    engineInjectIntoGames;
  const excludeListHasWildcard = !!engineExclude.match(/^[ \t]*\*[ \t]*$/m);

  return (
    <SettingsWrapper data-testid="settings-page">
      <SettingsList itemLayout="vertical" split={false}>
        <List.Item>
          <SettingsListItemMeta
            title={t('settings.language.title')}
            description={
              <>
                <div>{t('settings.language.description')}</div>
                <div>
                  <Trans
                    t={t}
                    i18nKey="settings.language.contribute"
                    components={[
                      <a href="https://github.com/ramensoftware/windhawk/wiki/translations">
                        website
                      </a>,
                    ]}
                  />
                </div>
              </>
            }
          />
          <SettingsSelect
            showSearch
            optionFilterProp="children"
            value={appLanguage}
            onChange={(value) => {
              updateAppSettings({
                appSettings: {
                  language: typeof value === 'string' ? value : 'en',
                },
              });
            }}
            dropdownMatchSelectWidth={false}
          >
            {appLanguages.map(([languageId, languageDisplayName]) => (
              <Select.Option key={languageId} value={languageId}>
                {languageDisplayName}
              </Select.Option>
            ))}
          </SettingsSelect>
          {appLanguage !== 'en' && (
            <SettingsNotice>
              <Trans
                t={t}
                i18nKey="settings.language.credits"
                components={(() => {
                  const links: React.ReactElement[] = [];
                  // creditsLink -> <0>, creditsLink1 -> <1>, ..., creditsLink9 -> <9>
                  for (let i = 0; i <= 9; i++) {
                    const key = i === 0 ? 'creditsLink' : `creditsLink${i}`;
                    const url = t(`settings.language.${key}`, {
                      defaultValue: '',
                    }) as string;
                    if (url) {
                      links.push(
                        <a key={key} href={sanitizeUrl(url)}>
                          link
                        </a>
                      );
                    } else {
                      break;
                    }
                  }
                  return links;
                })()}
              />
            </SettingsNotice>
          )}
        </List.Item>
        <List.Item>
          <SettingsListItemMeta
            title={t('settings.theme.title')}
            description={t('settings.theme.description')}
          />
          <Segmented
            value={theme}
            onChange={(value) => {
              setTheme(value === 'light' || value === 'auto' ? value : 'dark');
            }}
            options={[
              { label: t('settings.theme.dark'), value: 'dark' },
              { label: t('settings.theme.light'), value: 'light' },
              { label: t('settings.theme.system'), value: 'auto' },
            ]}
          />
        </List.Item>
        <List.Item data-testid="app-setting" data-setting-key="disableUpdateCheck">
          <SettingsListItemMeta
            title={t('settings.updates.title')}
            description={t('settings.updates.description')}
          />
          <Switch
            data-testid="app-setting-switch"
            checked={!appSettings.disableUpdateCheck}
            onChange={(checked) => {
              updateAppSettings({
                appSettings: {
                  disableUpdateCheck: !checked,
                },
              });
            }}
          />
        </List.Item>
        <List.Item data-testid="app-setting" data-setting-key="devModeOptOut">
          <SettingsListItemMeta
            title={t('settings.devMode.title')}
            description={t('settings.devMode.description')}
          />
          <Switch
            data-testid="app-setting-switch"
            checked={!appSettings.devModeOptOut}
            onChange={(checked) => {
              updateAppSettings({
                appSettings: {
                  devModeOptOut: !checked,
                },
              });
            }}
          />
        </List.Item>
        <List.Item>
          <SettingsListItemMeta
            title={t('settings.userData.title')}
            description={t('settings.userData.description')}
          />
          <UserDataSection onImported={refreshAppSettings} />
        </List.Item>
      </SettingsList>
      <Collapse>
        <Collapse.Panel header={
          <span data-testid="settings-advanced">
            {t('settings.advancedSettings')}
            {' '}
            {loggingEnabled && (
              <Tooltip title={t('general.status.loggingEnabled')} placement="bottom">
                <Badge dot status="warning" />
              </Tooltip>
            )}
          </span>
        } key="1">
          <List itemLayout="vertical" split={false}>
            <List.Item data-testid="app-setting" data-setting-key="hideTrayIcon">
              <SettingsListItemMeta
                title={t('settings.hideTrayIcon.title')}
                description={t('settings.hideTrayIcon.description')}
              />
              <Switch
                data-testid="app-setting-switch"
                checked={appSettings.hideTrayIcon}
                onChange={(checked) => {
                  updateAppSettings({
                    appSettings: {
                      hideTrayIcon: checked,
                    },
                  });
                }}
              />
            </List.Item>
            <List.Item
              data-testid="app-setting"
              data-setting-key="alwaysCompileModsLocally"
            >
              <SettingsListItemMeta
                title={t('settings.alwaysCompileModsLocally.title')}
                description={t('settings.alwaysCompileModsLocally.description')}
              />
              <Switch
                data-testid="app-setting-switch"
                checked={appSettings.alwaysCompileModsLocally}
                onChange={(checked) => {
                  updateAppSettings({
                    appSettings: {
                      alwaysCompileModsLocally: checked,
                    },
                  });
                }}
              />
            </List.Item>
            {/* Null in portable mode (no scheduled task there); hide the row. */}
            {appSettings.disableRunUIScheduledTask !== null && (
              <List.Item
                data-testid="app-setting"
                data-setting-key="disableRunUIScheduledTask"
              >
                <SettingsListItemMeta
                  title={t('settings.requireElevation.title')}
                  description={t('settings.requireElevation.description')}
                />
                <Switch
                  data-testid="app-setting-switch"
                  checked={appSettings.disableRunUIScheduledTask}
                  onChange={(checked) => {
                    updateAppSettings({
                      appSettings: {
                        disableRunUIScheduledTask: checked,
                      },
                    });
                  }}
                />
              </List.Item>
            )}
            <List.Item
              data-testid="app-setting"
              data-setting-key="dontAutoShowToolkit"
            >
              <SettingsListItemMeta
                title={t('settings.dontAutoShowToolkit.title')}
                description={t('settings.dontAutoShowToolkit.description')}
              />
              <Switch
                data-testid="app-setting-switch"
                checked={appSettings.dontAutoShowToolkit}
                onChange={(checked) => {
                  updateAppSettings({
                    appSettings: {
                      dontAutoShowToolkit: checked,
                    },
                  });
                }}
              />
            </List.Item>
            <List.Item
              data-testid="app-setting"
              data-setting-key="modTasksDialogDelay"
            >
              <SettingsListItemMeta
                title={t('settings.modInitDialogDelay.title')}
                description={t('settings.modInitDialogDelay.description')}
              />
              <SettingInputNumber
                // Add 1000 to the displayed value, since that's the amount of
                // extra delay that's actually added in the app.
                value={1000 + (appSettings.modTasksDialogDelay ?? 0)}
                min={1000 + 400}
                max={2147483647}
                onChange={(value) => {
                  updateAppSettings({
                    appSettings: {
                      modTasksDialogDelay: parseIntLax(value) - 1000,
                    },
                  });
                }}
              />
            </List.Item>
            <List.Item>
              <Badge
                dot={loggingEnabled}
                status="warning"
                title={loggingEnabled ? t('general.status.loggingEnabled') : undefined}
              >
                <Button
                  type="primary"
                  data-testid="settings-more-advanced"
                  onClick={() => {
                    resetMoreAdvancedSettings();
                    setIsMoreAdvancedSettingsModalOpen(true);
                  }}
                >
                  {t('settings.moreAdvancedSettings.title')}
                </Button>
              </Badge>
            </List.Item>
          </List>
        </Collapse.Panel>
      </Collapse>
      <Modal
        title={t('settings.moreAdvancedSettings.title')}
        open={isMoreAdvancedSettingsModalOpen}
        centered={true}
        bodyStyle={{ maxHeight: CSS.supports('height: 100dvh') ? '60dvh' : '60vh', overflow: 'auto' }}
        onOk={() => {
          updateAppSettings({
            appSettings: {
              loggingVerbosity: appLoggingVerbosity,
              engine: {
                loggingVerbosity: engineLoggingVerbosity,
                include: engineProcessListToArray(engineInclude),
                exclude: engineProcessListToArray(engineExclude),
                injectIntoCriticalProcesses: engineInjectIntoCriticalProcesses,
                injectIntoIncompatiblePrograms: engineInjectIntoIncompatiblePrograms,
                injectIntoGames: engineInjectIntoGames,
              },
            },
          });
          setIsMoreAdvancedSettingsModalOpen(false);
        }}
        onCancel={() => {
          setIsMoreAdvancedSettingsModalOpen(false);
        }}
        okText={t('settings.moreAdvancedSettings.saveButton')}
        okButtonProps={{
          type: 'primary',
          ...testIdProps('settings-more-advanced-save'),
        }}
        cancelText={t('general.actions.cancel')}
      >
        <List itemLayout="vertical" split={false}>
          <List.Item>
            <Alert
              description={t('settings.moreAdvancedSettings.restartNotice')}
              type="info"
              showIcon
            />
          </List.Item>
          <List.Item>
            <SettingsListItemMeta
              title={t('settings.loggingVerbosity.appLoggingTitle')}
              description={t('settings.loggingVerbosity.description')}
            />
            <SettingsSelect
              value={appLoggingVerbosity}
              onChange={(value) => {
                setAppLoggingVerbosity(typeof value === 'number' ? value : 0);
              }}
              dropdownMatchSelectWidth={false}
            >
              <Select.Option key="none" value={0}>
                {t('settings.loggingVerbosity.none')}
              </Select.Option>
              <Select.Option key="error" value={1}>
                {t('settings.loggingVerbosity.error')}
              </Select.Option>
              <Select.Option key="verbose" value={2}>
                {t('settings.loggingVerbosity.verbose')}
              </Select.Option>
            </SettingsSelect>
          </List.Item>
          <List.Item>
            <SettingsListItemMeta
              title={t('settings.loggingVerbosity.engineLoggingTitle')}
              description={t('settings.loggingVerbosity.description')}
            />
            <SettingsSelect
              value={engineLoggingVerbosity}
              onChange={(value) => {
                setEngineLoggingVerbosity(
                  typeof value === 'number' ? value : 0
                );
              }}
              dropdownMatchSelectWidth={false}
            >
              <Select.Option key="none" value={0}>
                {t('settings.loggingVerbosity.none')}
              </Select.Option>
              <Select.Option key="error" value={1}>
                {t('settings.loggingVerbosity.error')}
              </Select.Option>
              <Select.Option key="verbose" value={2}>
                {t('settings.loggingVerbosity.verbose')}
              </Select.Option>
            </SettingsSelect>
          </List.Item>
          <List.Item>
            <SettingsListItemMeta
              title={t('settings.processList.titleExclusion')}
              description={<>
                <p>{t('settings.processList.descriptionExclusion')}</p>
                <div>
                  <Trans
                    t={t}
                    i18nKey="settings.processList.descriptionExclusionWiki"
                    components={[<a href="https://github.com/ramensoftware/windhawk/wiki/Injection-targets-and-critical-system-processes">wiki</a>]}
                  />
                </div>
              </>}
            />
            <TextAreaWithContextMenu
              rows={4}
              data-testid="engine-exclude"
              value={engineExclude}
              placeholder={
                (t('settings.processList.processListPlaceholder') as string) +
                '\n' +
                'notepad.exe\n' +
                '%ProgramFiles%\\Notepad++\\notepad++.exe\n' +
                'C:\\Windows\\system32\\*'
              }
              onChange={(e) => {
                setEngineExclude(e.target.value);
              }}
            />
            {engineExclude.match(/["/<>|]/) && (
              <Alert
                description={t('settings.processList.invalidCharactersWarning', {
                  invalidCharacters: '" / < > |',
                })}
                type="warning"
                showIcon
              />
            )}
            <Space direction="vertical" size="small" style={{ marginTop: '12px' }}>
              <Checkbox
                checked={!engineInjectIntoCriticalProcesses}
                onChange={(e) => {
                  setEngineInjectIntoCriticalProcesses(!e.target.checked);
                }}
              >
                {t('settings.processList.excludeCriticalProcesses')}
              </Checkbox>
              <Checkbox
                checked={!engineInjectIntoIncompatiblePrograms}
                onChange={(e) => {
                  setEngineInjectIntoIncompatiblePrograms(!e.target.checked);
                }}
              >
                {t('settings.processList.excludeIncompatiblePrograms')}
              </Checkbox>
              <Checkbox
                checked={!engineInjectIntoGames}
                onChange={(e) => {
                  setEngineInjectIntoGames(!e.target.checked);
                }}
              >
                {t('settings.processList.excludeGames')}
              </Checkbox>
            </Space>
          </List.Item>
          <List.Item>
            <SettingsListItemMeta
              title={t('settings.processList.titleInclusion')}
              description={t('settings.processList.descriptionInclusion')}
            />
            <TextAreaWithContextMenu
              rows={4}
              data-testid="engine-include"
              value={engineInclude}
              placeholder={
                (t('settings.processList.processListPlaceholder') as string) +
                '\n' +
                'notepad.exe\n' +
                '%ProgramFiles%\\Notepad++\\notepad++.exe\n' +
                'C:\\Windows\\system32\\*'
              }
              onChange={(e) => {
                setEngineInclude(e.target.value);
              }}
            />
            {engineInclude.match(/["/<>|]/) && (
              <Alert
                description={t('settings.processList.invalidCharactersWarning', {
                  invalidCharacters: '" / < > |',
                })}
                type="warning"
                showIcon
              />
            )}
            {!includeListEmpty && excludeListEmpty && (
              <Alert
                description={t(
                  'settings.processList.inclusionWithoutExclusionNotice'
                )}
                type="warning"
                showIcon
              />
            )}
            {!includeListEmpty && !excludeListHasWildcard && (
              <Alert
                description={t(
                  'settings.processList.inclusionWithoutTotalExclusionNotice'
                )}
                type="info"
                showIcon
              />
            )}
          </List.Item>
        </List>
      </Modal>
    </SettingsWrapper>
  );
}

export default Settings;
