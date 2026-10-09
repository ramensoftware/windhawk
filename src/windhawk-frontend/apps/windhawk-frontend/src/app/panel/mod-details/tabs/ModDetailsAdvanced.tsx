import { faTrashCan } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import {
  Alert,
  Button,
  Dropdown,
  List,
  Modal,
  Select,
  Space,
  Switch,
  Tooltip,
} from 'antd';
import {
  type KeyboardEvent,
  useContext,
  useEffect,
  useRef,
  useState,
} from 'react';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { CurrentUserContext } from '@app/appUISettings';
import {
  PopconfirmModal,
  SelectModal,
  TextAreaWithContextMenu,
} from '@app/components/InputWithContextMenu';
import { showErrorMessage } from '@app/feedback';
import { useNavigationBlock } from '@app/navigationBlock';
import {
  showAdvancedDebugLogOutput,
  useDeleteModUserConfig,
  useSetModUserScope,
  useUpdateModConfig,
} from '@app/webviewIPC';
import { type ModConfig, type UserScope } from '@app/webviewIPCMessages';
import { useModSettings } from '../useModSettings';
import { type ModSettings } from './settings/core/yamlConverter';

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

function isModSettings(value: unknown): value is ModSettings {
  return (
    typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value) &&
    Object.values(value).every(
      (v) => typeof v === 'string' || typeof v === 'number'
    )
  );
}

// Whether a mod's values are read from the user's own configuration rather than
// the global one.
function readsUserConfig(userScope: UserScope | null | undefined) {
  return userScope === 'active';
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

// A trash button that asks before deleting. Its tooltip gives way to the
// confirmation, which opens below the same button.
function UserConfigDeleteButton(props: {
  disabled: boolean;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();

  const [confirmOpen, setConfirmOpen] = useState(false);
  const [tooltipOpen, setTooltipOpen] = useState(false);

  return (
    <PopconfirmModal
      placement="bottomLeft"
      arrowPointAtCenter
      // antd shifts the popup by a tooltip's arrow, which leaves a popover's
      // (16px in, 22px wide) off the button's center.
      align={{ offset: [-(16 + 22 / 2), 4] }}
      overlayStyle={{ maxWidth: 360 }}
      title={t('modDetails.advanced.userScope.deleteConfirm')}
      okText={t('modDetails.advanced.userScope.deleteConfirmOk')}
      cancelText={t('general.actions.cancel')}
      okButtonProps={{ danger: true, disabled: props.disabled }}
      onConfirm={props.onConfirm}
      open={confirmOpen}
      onOpenChange={(open) => {
        // antd hands a click on a disabled button in a tooltip to the
        // confirmation anyway, through the span it wraps the button in.
        if (open && props.disabled) {
          return;
        }
        setConfirmOpen(open);
        setTooltipOpen(false);
      }}
    >
      <Tooltip
        title={t('modDetails.advanced.userScope.delete')}
        placement="bottom"
        open={tooltipOpen}
        onOpenChange={(open) => setTooltipOpen(open && !confirmOpen)}
      >
        <Button
          type="text"
          size="small"
          icon={<FontAwesomeIcon icon={faTrashCan} />}
          disabled={props.disabled}
          aria-label={t('modDetails.advanced.userScope.delete')}
          data-testid="mod-user-config-delete"
        />
      </Tooltip>
    </PopconfirmModal>
  );
}

interface Props {
  modId: string;
  // The mod's config as the installed mods hold it, null for a mod with none,
  // such as a local mod never compiled.
  config: ModConfig | null;
}

function ModDetailsAdvanced({ modId, config }: Props) {
  const { t } = useTranslation();

  // The mod's settings as last read or saved, which the settings box shows
  // while it holds no edit, in the form the load menu last picked.
  const [savedModSettings, setSavedModSettings] = useState<ModSettings>();
  const [modSettingsIndented, setModSettingsIndented] = useState(false);
  // The settings box's text while it is being edited.
  const [modSettingsDraft, setModSettingsDraft] = useState<string>();
  // A custom list's text while it is being edited, undefined while the list
  // reads as the config has it.
  const [customIncludeDraft, setCustomIncludeDraft] = useState<string>();
  const [customExcludeDraft, setCustomExcludeDraft] = useState<string>();
  const [userScopeConfirmOpen, setUserScopeConfirmOpen] = useState(false);

  const currentUser = useContext(CurrentUserContext);

  const { reload: reloadModSettings, save: writeModSettings } = useModSettings(
    modId,
    { onRead: (settings) => setSavedModSettings(settings) }
  );
  const { updateModConfig } = useUpdateModConfig();
  const { setModUserScope, setModUserScopePending } = useSetModUserScope();
  const { deleteModUserConfig, deleteModUserConfigPending } =
    useDeleteModUserConfig();
  const userScopePending = setModUserScopePending || deleteModUserConfigPending;

  // Modal.confirm renders outside the route tree, so a route change would leave
  // the confirmation over another page, still able to switch the scope.
  useNavigationBlock(userScopeConfirmOpen);

  // Nor does it unmount with the tab, which would leave it switching the scope
  // for a tab that is gone.
  const userScopeConfirm = useRef<ReturnType<typeof Modal.confirm> | null>(
    null
  );
  useEffect(() => () => userScopeConfirm.current?.destroy(), []);

  // Every value on this tab but the scope belongs to the configuration in use,
  // so when another one comes into use, here or elsewhere, edits not saved yet
  // are dropped and the settings are read again; between none and inactive it
  // is the global one either way. The values stay drawn meanwhile, which keeps
  // the panel's scroll position.
  const fromUserConfig = readsUserConfig(config?.userScope);
  const [editsFromUserConfig, setEditsFromUserConfig] =
    useState(fromUserConfig);
  if (editsFromUserConfig !== fromUserConfig) {
    setEditsFromUserConfig(fromUserConfig);
    setModSettingsDraft(undefined);
    setCustomIncludeDraft(undefined);
    setCustomExcludeDraft(undefined);
  }

  // Read again for the configuration that comes into use; the first read is
  // the owner's own.
  const settingsReadFromUserConfig = useRef(fromUserConfig);
  useEffect(() => {
    if (settingsReadFromUserConfig.current !== fromUserConfig) {
      settingsReadFromUserConfig.current = fromUserConfig;
      reloadModSettings();
    }
  }, [fromUserConfig, reloadModSettings]);

  if (savedModSettings === undefined) {
    return null;
  }

  const modSettingsText =
    modSettingsDraft ??
    JSON.stringify(savedModSettings, null, modSettingsIndented ? 2 : undefined);

  // The load button's two entries: the settings as compact as the host sends
  // them, or indented.
  const loadModSettings = (indented: boolean) => {
    setModSettingsIndented(indented);
    setModSettingsDraft(undefined);
    reloadModSettings();
  };

  const debugLogging = config?.debugLoggingEnabled
    ? 2
    : config?.loggingEnabled
      ? 1
      : 0;
  const customInclude =
    customIncludeDraft ?? engineArrayToProcessList(config?.includeCustom ?? []);
  const customExclude =
    customExcludeDraft ?? engineArrayToProcessList(config?.excludeCustom ?? []);
  const userScope = config ? config.userScope : null;

  // Creating the user's configuration is explained first. Switching to one that
  // exists, or back to the global one, is not: the explanation was accepted
  // when it was created.
  const onUserScopeSwitch = (checked: boolean) => {
    if (!checked || userScope !== 'none') {
      void setModUserScope({ modId, active: checked });
      return;
    }
    setUserScopeConfirmOpen(true);
    userScopeConfirm.current = Modal.confirm({
      title: t('modDetails.advanced.userScope.confirmTitle'),
      content: (
        <>
          <p>{t('modDetails.advanced.userScope.confirmDescription')}</p>
          <p>{t('modDetails.advanced.userScope.confirmDescriptionOthers')}</p>
          {currentUser && (
            <p>
              {t('modDetails.advanced.userScope.currentUser', {
                userName: currentUser.name,
              })}
            </p>
          )}
        </>
      ),
      okText: t('modDetails.advanced.userScope.confirmOk'),
      cancelText: t('general.actions.cancel'),
      onOk: () => {
        userScopeConfirm.current = null;
        setUserScopeConfirmOpen(false);
        void setModUserScope({ modId, active: true });
      },
      onCancel: () => {
        userScopeConfirm.current = null;
        setUserScopeConfirmOpen(false);
      },
      closable: true,
      maskClosable: true,
    });
  };

  // A saved box shows the settings again, unless it was edited further while
  // the write was out.
  const saveModSettings = async () => {
    if (modSettingsDraft === undefined) {
      return;
    }
    const sent = modSettingsDraft;
    const trimmed = sent.trim();
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
    if (await writeModSettings(settings)) {
      setSavedModSettings(settings);
      setModSettingsDraft((draft) => (draft === sent ? undefined : draft));
    }
  };

  // A config write reaches the screen through `config`, moved by the host's echo
  // of the write, which lands before the reply. A refused write is echoed by
  // nothing, so its control keeps reading as what the host still holds.
  const saveDebugLogging = (level: number) =>
    updateModConfig({
      modId,
      config: {
        loggingEnabled: level === 1,
        debugLoggingEnabled: level === 2,
      },
    });

  // A saved list reads as the config has it again, unless it was edited further
  // while the write was out.
  const saveCustomInclude = async () => {
    if (customIncludeDraft === undefined) {
      return;
    }
    const result = await updateModConfig({
      modId,
      config: {
        includeCustom: engineProcessListToArray(customIncludeDraft),
      },
    });
    if (result.status === 'reply' && result.data.succeeded) {
      setCustomIncludeDraft((draft) =>
        draft === customIncludeDraft ? undefined : draft
      );
    }
  };

  const saveCustomExclude = async () => {
    if (customExcludeDraft === undefined) {
      return;
    }
    const result = await updateModConfig({
      modId,
      config: {
        excludeCustom: engineProcessListToArray(customExcludeDraft),
      },
    });
    if (result.status === 'reply' && result.data.succeeded) {
      setCustomExcludeDraft((draft) =>
        draft === customExcludeDraft ? undefined : draft
      );
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
            value={modSettingsText}
            onChange={(e) => setModSettingsDraft(e.target.value)}
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
                onClick: (e) => loadModSettings(e.key === 'formatted'),
              }}
              onClick={() => loadModSettings(false)}
            >
              {t('modDetails.advanced.modSettings.loadButton')}
            </Dropdown.Button>
            <Button
              type="primary"
              disabled={modSettingsDraft === undefined}
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
              onChange={(e) => setCustomIncludeDraft(e.target.value)}
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
            disabled={customIncludeDraft === undefined}
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
              onChange={(e) => setCustomExcludeDraft(e.target.value)}
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
            disabled={customExcludeDraft === undefined}
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
          checked={config?.includeExcludeCustomOnly ?? false}
          onChange={(checked) => {
            void updateModConfig({
              modId,
              config: { includeExcludeCustomOnly: checked },
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
          checked={config?.patternsMatchCriticalSystemProcesses ?? false}
          onChange={(checked) => {
            void updateModConfig({
              modId,
              config: { patternsMatchCriticalSystemProcesses: checked },
            });
          }}
        />
      </List.Item>
      {/* A host that names no account, as a portable install's does, has no
          user configurations to offer; one that already exists stays reachable.
          A mod with no config has nothing to copy into one. */}
      {userScope !== null && (currentUser || userScope !== 'none') && (
        <List.Item>
          <SettingsListItemMeta
            title={t('modDetails.advanced.userScope.title')}
            description={
              <>
                {t('modDetails.advanced.userScope.description')}
                {currentUser && (
                  <div>
                    {t('modDetails.advanced.userScope.currentUser', {
                      userName: currentUser.name,
                    })}
                  </div>
                )}
              </>
            }
          />
          <Space>
            <Switch
              checked={userScope === 'active'}
              loading={userScopePending}
              data-testid="mod-user-scope-switch"
              onChange={onUserScopeSwitch}
            />
            {userScope !== 'none' && (
              <UserConfigDeleteButton
                disabled={userScopePending}
                onConfirm={() => {
                  void deleteModUserConfig({ modId });
                }}
              />
            )}
          </Space>
        </List.Item>
      )}
    </List>
  );
}

export default ModDetailsAdvanced;
