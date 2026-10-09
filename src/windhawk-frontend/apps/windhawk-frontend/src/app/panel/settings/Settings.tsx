import { AppUISettingsContext } from '@app/appUISettings';
import { InputNumberWithContextMenu, SelectModal, TextAreaWithContextMenu } from '@app/components/InputWithContextMenu';
import { appLanguages } from '@app/constants/languages';
import { isWireError } from '@app/feedback';
import { useNavigationBlock } from '@app/navigationBlock';
import { useReadWriteGuard } from '@app/panel/shared/useReadWriteGuard';
import { useResync } from '@app/panel/shared/useResync';
import { useUnsavedChangesPrompt } from '@app/panel/shared/useUnsavedChangesPrompt';
import { useTheme } from '@app/theme';
import { replaceEqualDeep, sanitizeUrl, testIdProps } from '@app/utils';
import { useGetAppSettings, useUpdateAppSettings } from '@app/webviewIPC';
import { type AppSettings } from '@app/webviewIPCMessages';
import { Alert, Badge, Button, Checkbox, Collapse, List, Modal, Segmented, Select, Space, Switch, Tooltip } from 'antd';
import { useCallback, useContext, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';

import { UserDataSection } from './userData';

const SettingsWrapper = styled.div`
  padding-bottom: 20px;
`;

const SettingsList = styled(List)`
  margin-bottom: 20px;
`;

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

// What the more-advanced dialog edits, in the form its fields hold. The saved
// settings are drawn in this shape and the dialog's edits are compared against
// them in it, so it can tell whether it is holding edits that leaving would throw
// away.
type MoreAdvancedSettings = {
  appLoggingVerbosity: number;
  engineLoggingVerbosity: number;
  engineInclude: string;
  engineExclude: string;
  engineInjectIntoCriticalProcesses: boolean;
  engineInjectIntoIncompatiblePrograms: boolean;
  engineInjectIntoGames: boolean;
};

function savedMoreAdvancedSettings(
  appSettings: Partial<AppSettings> | null
): MoreAdvancedSettings {
  return {
    appLoggingVerbosity: appSettings?.loggingVerbosity ?? 0,
    engineLoggingVerbosity: appSettings?.engine?.loggingVerbosity ?? 0,
    engineInclude: engineArrayToProcessList(appSettings?.engine?.include ?? []),
    engineExclude: engineArrayToProcessList(appSettings?.engine?.exclude ?? []),
    engineInjectIntoCriticalProcesses:
      appSettings?.engine?.injectIntoCriticalProcesses ?? false,
    engineInjectIntoIncompatiblePrograms:
      appSettings?.engine?.injectIntoIncompatiblePrograms ?? false,
    engineInjectIntoGames: appSettings?.engine?.injectIntoGames ?? false,
  };
}

// The edits that differ from the saved settings: one that matches them, typed
// back or read in, is no edit.
function unsavedEdits(
  edits: Partial<MoreAdvancedSettings>,
  saved: MoreAdvancedSettings
) {
  return Object.fromEntries(
    Object.entries(edits).filter(
      ([key, value]) => value !== saved[key as keyof MoreAdvancedSettings]
    )
  ) as Partial<MoreAdvancedSettings>;
}

function Settings() {
  const { t, i18n } = useTranslation();
  const appLanguage = i18n.resolvedLanguage;

  const { loggingEnabled } = useContext(AppUISettingsContext);

  const { theme, setTheme } = useTheme();

  const [appSettings, setAppSettings] = useState<Partial<AppSettings> | null>(
    null
  );

  // The more-advanced dialog's edits, field by field. A field without one reads
  // as the saved settings have it, and so follows them when they are read again
  // with the dialog open.
  const [advancedEdits, setAdvancedEdits] = useState<
    Partial<MoreAdvancedSettings>
  >({});
  const advancedSaved = savedMoreAdvancedSettings(appSettings);
  const advanced: MoreAdvancedSettings = { ...advancedSaved, ...advancedEdits };

  // Held against the saved settings as they move: an edit they come to match is
  // dropped, so the field follows them from there.
  const [editsHeldAgainst, setEditsHeldAgainst] = useState(appSettings);
  if (editsHeldAgainst !== appSettings) {
    setEditsHeldAgainst(appSettings);
    setAdvancedEdits((edits) => unsavedEdits(edits, advancedSaved));
  }

  const editAdvanced = <K extends keyof MoreAdvancedSettings>(
    key: K,
    value: MoreAdvancedSettings[K]
  ) => {
    setAdvancedEdits((edits) =>
      unsavedEdits({ ...edits, [key]: value }, advancedSaved)
    );
  };

  const { getAppSettings } = useGetAppSettings();
  const { updateAppSettings } = useUpdateAppSettings();
  const { mark, write, overtaken } = useReadWriteGuard();

  // The settings as the machine holds them, read on mount, when the window comes
  // back (the tray, another window or the CLI can write them meanwhile) and when
  // an import has written them, all through the one queue. A read is applied
  // only where no write of this page's overtook it (see useReadWriteGuard). The
  // page draws nothing before its first read, so no write goes out ahead of it.
  const readAppSettings = useCallback(async () => {
    const at = mark();
    const result = await getAppSettings({});
    if (result.status !== 'reply' || overtaken(at)) {
      return;
    }
    // A read the host could not make carries an empty object, which says nothing
    // of the settings the page already draws.
    const { appSettings: read, error } = result.data;
    setAppSettings((held) =>
      held && isWireError(error) ? held : replaceEqualDeep(held, read)
    );
  }, [getAppSettings, mark, overtaken]);

  const reloadAppSettings = useResync(readAppSettings, { onMount: true });

  // A write carries only the fields it changes, and its reply echoes them. Merged
  // into the settings as they stand, not into the ones the write was posted over:
  // two replies can land in one batch, before a render has put either into that
  // copy.
  const writeAppSettings = useCallback(
    async (written: Partial<AppSettings>) => {
      const result = await write(() =>
        updateAppSettings({ appSettings: written })
      );
      if (result.status === 'reply' && result.data.succeeded) {
        setAppSettings((saved) =>
          saved
            ? {
                ...saved,
                ...result.data.appSettings,
              }
            : saved
        );
      }
    },
    [write, updateAppSettings]
  );

  const [isMoreAdvancedSettingsModalOpen, setIsMoreAdvancedSettingsModalOpen] =
    useState(false);

  // The dialog's fields are only committed by its Save button, so a route change
  // taken with edits in them (the mask over the page leaves Alt+Left as the way
  // out) would drop the lot. Ask rather than refuse: giving them up is a fair
  // thing to want.
  const moreAdvancedSettingsEdited =
    isMoreAdvancedSettingsModalOpen && Object.keys(advancedEdits).length > 0;

  const confirmDiscardMoreAdvancedSettings = useUnsavedChangesPrompt({
    title: t('settings.moreAdvancedSettings.unsavedChangesTitle') as string,
    message: t('settings.moreAdvancedSettings.unsavedChangesMessage') as string,
    leave: t('settings.moreAdvancedSettings.unsavedChangesLeave') as string,
    stay: t('settings.moreAdvancedSettings.unsavedChangesStay') as string,
  });

  useNavigationBlock(
    moreAdvancedSettingsEdited,
    confirmDiscardMoreAdvancedSettings
  );

  if (!appSettings) {
    return null;
  }

  const includeListEmpty = advanced.engineInclude.trim() === '';
  const excludeListEmpty = advanced.engineExclude.trim() === '' &&
    advanced.engineInjectIntoCriticalProcesses &&
    advanced.engineInjectIntoIncompatiblePrograms &&
    advanced.engineInjectIntoGames;
  const excludeListHasWildcard = !!advanced.engineExclude.match(/^[ \t]*\*[ \t]*$/m);

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
              void writeAppSettings({
                language: typeof value === 'string' ? value : 'en',
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
              { label: t('settings.theme.dark'), value: 'dark', title: '' },
              { label: t('settings.theme.light'), value: 'light', title: '' },
              { label: t('settings.theme.system'), value: 'auto', title: '' },
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
              void writeAppSettings({ disableUpdateCheck: !checked });
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
              void writeAppSettings({ devModeOptOut: !checked });
            }}
          />
        </List.Item>
        <List.Item>
          <SettingsListItemMeta
            title={t('settings.userData.title')}
            description={t('settings.userData.description')}
          />
          <UserDataSection onImported={reloadAppSettings} />
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
                  void writeAppSettings({ hideTrayIcon: checked });
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
                  void writeAppSettings({ alwaysCompileModsLocally: checked });
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
                    void writeAppSettings({
                      disableRunUIScheduledTask: checked,
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
                  void writeAppSettings({ dontAutoShowToolkit: checked });
                }}
              />
            </List.Item>
            <List.Item
              data-testid="app-setting"
              data-setting-key="disableToolkitHotkey"
            >
              <SettingsListItemMeta
                title={t('settings.disableToolkitHotkey.title')}
                description={t('settings.disableToolkitHotkey.description')}
              />
              <Switch
                data-testid="app-setting-switch"
                checked={appSettings.disableToolkitHotkey}
                onChange={(checked) => {
                  void writeAppSettings({ disableToolkitHotkey: checked });
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
                  // An emptied field reports null, the one value the field lets
                  // through without a range check. Taking the offset off it
                  // would store -1000, far below the minimum declared above, so
                  // an edit only commits once there is a number back in it.
                  if (value === null || value === undefined) {
                    return;
                  }

                  void writeAppSettings({
                    modTasksDialogDelay: parseIntLax(value) - 1000,
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
                    setAdvancedEdits({});
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
          void writeAppSettings({
            loggingVerbosity: advanced.appLoggingVerbosity,
            engine: {
              loggingVerbosity: advanced.engineLoggingVerbosity,
              include: engineProcessListToArray(advanced.engineInclude),
              exclude: engineProcessListToArray(advanced.engineExclude),
              injectIntoCriticalProcesses: advanced.engineInjectIntoCriticalProcesses,
              injectIntoIncompatiblePrograms: advanced.engineInjectIntoIncompatiblePrograms,
              injectIntoGames: advanced.engineInjectIntoGames,
            },
          });
          setIsMoreAdvancedSettingsModalOpen(false);
        }}
        onCancel={() => {
          setIsMoreAdvancedSettingsModalOpen(false);
        }}
        maskClosable={false}
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
              value={advanced.appLoggingVerbosity}
              onChange={(value) => {
                editAdvanced(
                  'appLoggingVerbosity',
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
              title={t('settings.loggingVerbosity.engineLoggingTitle')}
              description={t('settings.loggingVerbosity.description')}
            />
            <SettingsSelect
              value={advanced.engineLoggingVerbosity}
              onChange={(value) => {
                editAdvanced(
                  'engineLoggingVerbosity',
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
              value={advanced.engineExclude}
              placeholder={
                (t('settings.processList.processListPlaceholder') as string) +
                '\n' +
                'notepad.exe\n' +
                '%ProgramFiles%\\Notepad++\\notepad++.exe\n' +
                'C:\\Windows\\system32\\*'
              }
              onChange={(e) => {
                editAdvanced('engineExclude', e.target.value);
              }}
            />
            {advanced.engineExclude.match(/["/<>|]/) && (
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
                checked={!advanced.engineInjectIntoCriticalProcesses}
                onChange={(e) => {
                  editAdvanced('engineInjectIntoCriticalProcesses', !e.target.checked);
                }}
              >
                {t('settings.processList.excludeCriticalProcesses')}
              </Checkbox>
              <Checkbox
                checked={!advanced.engineInjectIntoIncompatiblePrograms}
                onChange={(e) => {
                  editAdvanced('engineInjectIntoIncompatiblePrograms', !e.target.checked);
                }}
              >
                {t('settings.processList.excludeIncompatiblePrograms')}
              </Checkbox>
              <Checkbox
                checked={!advanced.engineInjectIntoGames}
                onChange={(e) => {
                  editAdvanced('engineInjectIntoGames', !e.target.checked);
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
              value={advanced.engineInclude}
              placeholder={
                (t('settings.processList.processListPlaceholder') as string) +
                '\n' +
                'notepad.exe\n' +
                '%ProgramFiles%\\Notepad++\\notepad++.exe\n' +
                'C:\\Windows\\system32\\*'
              }
              onChange={(e) => {
                editAdvanced('engineInclude', e.target.value);
              }}
            />
            {advanced.engineInclude.match(/["/<>|]/) && (
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
