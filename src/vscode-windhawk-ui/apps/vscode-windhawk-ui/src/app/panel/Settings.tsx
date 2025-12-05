import { Alert, Badge, Button, Checkbox, Collapse, List, Modal, Select, Space, Switch, Tooltip } from 'antd';
import { useCallback, useContext, useEffect, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { AppUISettingsContext } from '../appUISettings';
import { InputNumberWithContextMenu, SelectModal, TextAreaWithContextMenu } from '../components/InputWithContextMenu';
import { sanitizeUrl } from '../utils';
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
    ar: 'العربية',
    cs: 'Čeština',
    da: 'Dansk',
    de: 'Deutsch',
    el: 'Ελληνικά',
    es: 'Español',
    fr: 'Français',
    hi: 'हिन्दी',
    hr: 'Hrvatski',
    hu: 'Magyar',
    id: 'Bahasa Indonesia',
    it: 'Italiano',
    ja: '日本語',
    ko: '한국어',
    nl: 'Nederlands',
    pl: 'Polski',
    'pt-BR': 'Português',
    ro: 'Română',
    ru: 'Русский',
    sv: 'Svenska',
    ta: 'தமிழ்',
    th: 'ภาษาไทย',
    tr: 'Türkçe',
    uk: 'Українська',
    vi: 'Tiếng Việt',
    'zh-CN': '简体中文',
    'zh-TW': '繁體中文',
  }).sort((a, b) => a[1].localeCompare(b[1])),
];

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

  const [appSettings, setAppSettings] = useState<Partial<AppSettings> | null>(
    mockSettings
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
                  <a href={sanitizeUrl(t('settings.language.creditsLink') as string)}>
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
        <Collapse.Panel header={
          <>
            {t('settings.advancedSettings')}
            {' '}
            {loggingEnabled && (
              <Tooltip title={t('general.loggingEnabled')} placement="bottom">
                <Badge dot status="warning" />
              </Tooltip>
            )}
          </>
        } key="1">
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
            <List.Item>
              <SettingsListItemMeta
                title={t('settings.alwaysCompileModsLocally.title')}
                description={t('settings.alwaysCompileModsLocally.description')}
              />
              <Switch
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
                title={loggingEnabled ? t('general.loggingEnabled') : undefined}
              >
                <Button
                  type="primary"
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
        bodyStyle={{ maxHeight: '60vh', overflow: 'auto' }}
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
