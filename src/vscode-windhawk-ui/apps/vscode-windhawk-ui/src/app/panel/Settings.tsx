import { Alert, Button, Collapse, List, Modal, Select, Switch } from 'antd';
import { useCallback, useEffect, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { InputNumberWithContextMenu, SelectModal, TextAreaWithContextMenu } from '../components/InputWithContextMenu';
import { useGetAppSettings, useUpdateAppSettings } from '../webviewIPC';
import { AppSettings } from '../webviewIPCMessages';
import { mockSettings } from './mockData';

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
  color: rgba(255, 255, 255, 0.45);
`;

const SettingInputNumber = styled(InputNumberWithContextMenu)`
  width: 100%;
  max-width: 130px;

  // Remove default VSCode focus highlighting color.
  input:focus {
    outline: none !important;
  }
`;

const appLanguages = [
  ['en', 'English'],
  ...Object.entries({
    de: 'Deutsch',
    es: 'Español',
    fr: 'Français',
    it: 'Italiano',
    ja: '日本語',
    ko: '한국어',
    'nl-NL': 'Nederlands',
    pl: 'Polski',
    'pt-BR': 'Português',
    ro: 'Română',
    ru: 'Русский',
    'sv-SE': 'Svenska',
    tr: 'Türkçe',
    uk: 'Українська',
    'zh-CN': '简体中文',
    'zh-TW': '繁體中文',
  }).sort((a, b) => a[1].localeCompare(b[1])),
];

function parseIntLax(value?: string | number | null) {
  const result = parseInt((value ?? 0).toString(), 10);
  return Number.isNaN(result) ? 0 : result;
}

function engineIncludeFromEngineAppSettings(
  engineAppSettings: AppSettings['engine']
) {
  return engineAppSettings.include.join('\n');
}

function engineExcludeFromEngineAppSettings(
  engineAppSettings: AppSettings['engine']
) {
  return [
    ...(engineAppSettings.injectIntoCriticalProcesses
      ? []
      : ['<critical_system_processes>']),
    ...engineAppSettings.exclude,
  ].join('\n');
}

function engineIncludeToEngineAppSettings(include: string) {
  const includeArray = include
    .split('\n')
    .map((x) => x.trim())
    .filter((x) => x);
  return {
    include: includeArray,
  };
}

function engineExcludeToEngineAppSettings(exclude: string) {
  const excludeArray = exclude
    .split('\n')
    .map((x) => x.trim())
    .filter((x) => x);
  return {
    exclude: excludeArray.filter((x) => x !== '<critical_system_processes>'),
    injectIntoCriticalProcesses: !excludeArray.includes(
      '<critical_system_processes>'
    ),
  };
}

function Settings() {
  const { t, i18n } = useTranslation();
  const appLanguage = i18n.resolvedLanguage;

  const [appSettings, setAppSettings] = useState<AppSettings | null>(
    mockSettings
  );

  // More advanced settings.
  const [appLoggingVerbosity, setAppLoggingVerbosity] = useState(0);
  const [engineLoggingVerbosity, setEngineLoggingVerbosity] = useState(0);
  const [engineInclude, setEngineInclude] = useState('');
  const [engineExclude, setEngineExclude] = useState('');
  const [
    engineLoadModsInCriticalSystemProcesses,
    setEngineLoadModsInCriticalSystemProcesses,
  ] = useState(0);

  const resetMoreAdvancedSettings = useCallback(() => {
    if (appSettings) {
      setAppLoggingVerbosity(appSettings.loggingVerbosity);
      setEngineLoggingVerbosity(appSettings.engine.loggingVerbosity);
      setEngineInclude(engineIncludeFromEngineAppSettings(appSettings.engine));
      setEngineExclude(engineExcludeFromEngineAppSettings(appSettings.engine));
      setEngineLoadModsInCriticalSystemProcesses(
        appSettings.engine.loadModsInCriticalSystemProcesses
      );
    }
  }, [appSettings]);

  const { getAppSettings } = useGetAppSettings(
    useCallback((data) => {
      setAppSettings(data.appSettings);
    }, [])
  );

  useEffect(() => {
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

  const excludeListHasCriticalSystemProcesses = !!engineExclude.match(
    /^[ \t]*<critical_system_processes>[ \t]*$/m
  );
  const includeListEmpty = engineInclude.trim() === '';
  const excludeListEmpty = engineExclude.trim() === '';
  const excludeListHasWildcard =
    !excludeListEmpty && !!engineExclude.match(/^[ \t]*\*[ \t]*$/m);

  if (!appSettings) {
    return null;
  }

  return (
    <SettingsWrapper>
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
                components={[
                  <a href={t('settings.language.creditsLink') as string}>
                    website
                  </a>,
                ]}
              />
            </SettingsNotice>
          )}
        </List.Item>
        <List.Item>
          <SettingsListItemMeta
            title={t('settings.updates.title')}
            description={t('settings.updates.description')}
          />
          <Switch
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
        <List.Item>
          <SettingsListItemMeta
            title={t('settings.devMode.title')}
            description={t('settings.devMode.description')}
          />
          <Switch
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
      </SettingsList>
      <Collapse>
        <Collapse.Panel header={t('settings.advancedSettings')} key="1">
          <List itemLayout="vertical" split={false}>
            <List.Item>
              <SettingsListItemMeta
                title={t('settings.hideTrayIcon.title')}
                description={t('settings.hideTrayIcon.description')}
              />
              <Switch
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
            {appSettings.disableRunUIScheduledTask !== null && (
              <List.Item>
                <SettingsListItemMeta
                  title={t('settings.requireElevation.title')}
                  description={t('settings.requireElevation.description')}
                />
                <Switch
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
            <List.Item>
              <SettingsListItemMeta
                title={t('settings.dontAutoShowToolkit.title')}
                description={t('settings.dontAutoShowToolkit.description')}
              />
              <Switch
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
            <List.Item>
              <SettingsListItemMeta
                title={t('settings.modInitDialogDelay.title')}
                description={t('settings.modInitDialogDelay.description')}
              />
              <SettingInputNumber
                // Add 1000 to the displayed value, since that's the amount of
                // extra delay that's actually added in the app.
                value={1000 + appSettings.modTasksDialogDelay}
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
              <Button
                type="primary"
                onClick={() => {
                  resetMoreAdvancedSettings();
                  setIsMoreAdvancedSettingsModalOpen(true);
                }}
              >
                {t('settings.moreAdvancedSettings.title')}
              </Button>
            </List.Item>
          </List>
        </Collapse.Panel>
      </Collapse>
      <Modal
        title={t('settings.moreAdvancedSettings.title')}
        open={isMoreAdvancedSettingsModalOpen}
        centered={true}
        bodyStyle={{ maxHeight: '60vh', overflow: 'auto' }}
        onOk={() => {
          updateAppSettings({
            appSettings: {
              loggingVerbosity: appLoggingVerbosity,
              engine: {
                loggingVerbosity: engineLoggingVerbosity,
                ...engineIncludeToEngineAppSettings(engineInclude),
                ...engineExcludeToEngineAppSettings(engineExclude),
                loadModsInCriticalSystemProcesses:
                  engineLoadModsInCriticalSystemProcesses,
              },
            },
          });
          setIsMoreAdvancedSettingsModalOpen(false);
        }}
        onCancel={() => {
          setIsMoreAdvancedSettingsModalOpen(false);
        }}
        okText={t('settings.moreAdvancedSettings.saveButton')}
        cancelText={t('settings.moreAdvancedSettings.cancelButton')}
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
              description={t('settings.processList.descriptionExclusion')}
            />
            <TextAreaWithContextMenu
              rows={4}
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
            {!excludeListHasCriticalSystemProcesses && (
              <Alert
                description={
                  <Trans
                    t={t}
                    i18nKey="settings.processList.exclusionCriticalProcessesNotice"
                    components={[<code />]}
                    values={{
                      critical_system_processes: '<critical_system_processes>',
                    }}
                    // A workaround to make a value with `<` work properly.
                    shouldUnescape
                    tOptions={{
                      interpolation: {
                        escapeValue: true,
                      },
                    }}
                  />
                }
                type="info"
                showIcon
              />
            )}
          </List.Item>
          <List.Item>
            <SettingsListItemMeta
              title={t('settings.processList.titleInclusion')}
              description={t('settings.processList.descriptionInclusion')}
            />
            <TextAreaWithContextMenu
              rows={4}
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
          <List.Item>
            <SettingsListItemMeta
              title={t('settings.loadModsInCriticalSystemProcesses.title')}
              description={t(
                'settings.loadModsInCriticalSystemProcesses.description'
              )}
            />
            <SettingsSelect
              value={engineLoadModsInCriticalSystemProcesses}
              onChange={(value) => {
                setEngineLoadModsInCriticalSystemProcesses(
                  typeof value === 'number' ? value : 0
                );
              }}
              dropdownMatchSelectWidth={false}
            >
              <Select.Option key="never" value={0}>
                {t('settings.loadModsInCriticalSystemProcesses.never')}
              </Select.Option>
              <Select.Option key="onlyExplicitMatch" value={1}>
                {t(
                  'settings.loadModsInCriticalSystemProcesses.onlyExplicitMatch'
                )}
              </Select.Option>
              <Select.Option key="always" value={2}>
                {t('settings.loadModsInCriticalSystemProcesses.always')}
              </Select.Option>
            </SettingsSelect>
          </List.Item>
        </List>
      </Modal>
    </SettingsWrapper>
  );
}

export default Settings;
