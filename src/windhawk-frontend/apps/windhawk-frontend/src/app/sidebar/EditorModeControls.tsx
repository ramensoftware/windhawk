import { Badge, Button, Dropdown, Switch, Tooltip } from 'antd';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { PopconfirmModal } from '../components/InputWithContextMenu';
import {
  previewEditedMod,
  showLogOutput,
  stopCompileEditedMod,
  useCompileEditedMod,
  useCompileEditedModStart,
  useDeleteEditedMod,
  useEditedModWasModified,
  useEnableEditedMod,
  useEnableEditedModLogging,
  useExitEditorMode,
  useSetEditedModDetails,
  useSetEditedModId,
} from '../webviewIPC';

const SidebarContainer = styled.div`
  padding: 0 10px;
  text-align: center;
`;

const SwitchesContainer = styled.div`
  margin-bottom: 10px;

  > * {
    width: 100%;
    display: flex;
    justify-content: space-between;
    background-color: var(--vscode-editor-background);
    border: 1px solid var(--whui-border);
    padding: 4px 10px;
  }

  > *:not(:last-child) {
    border-bottom: none;
  }

  > *:first-child {
    border-top-left-radius: 2px;
    border-top-right-radius: 2px;
  }

  > *:last-child {
    border-bottom-left-radius: 2px;
    border-bottom-right-radius: 2px;
  }
`;

const SwitchesContainerRow = styled.div`
  // Fixes a button alignment bug.
  > .ant-tooltip-disabled-compatible-wrapper {
    font-size: 0;
  }
`;

const ButtonsContainer = styled.div`
  > * {
    margin-bottom: 10px;
  }
`;

const ModIdBox = styled.div`
  display: inline-block;
  border-radius: 2px;
  background: var(--whui-chip-bg);
  padding: 1px 4px;
  overflow-wrap: anywhere;
  margin-bottom: 10px;
`;

const CompileButtonBadge = styled(Badge)`
  display: block;
  cursor: default;

  // Fixes badge z-index issue with dropdown button.
  > .ant-scroll-number {
    z-index: 3;
  }
`;

const FullWidthDropdownButton = styled(Dropdown.Button)`
  .ant-btn:not(.ant-dropdown-trigger) {
    width: 100%;
  }
`;

type ModDetailsCommon = {
  modId: string;
  modWasModified: boolean;
  noWindhawkExitButton: boolean;
};

type ModDetailsNotCompiled = ModDetailsCommon & {
  compiled: false;
};

type ModDetailsCompiled = ModDetailsCommon & {
  compiled: true;
  disabled: boolean;
  loggingEnabled: boolean;
  debugLoggingEnabled: boolean;
};

export type ModDetails = ModDetailsNotCompiled | ModDetailsCompiled;

interface Props {
  initialModDetails: ModDetails;
  onExitEditorMode?: () => void;
}

function EditorModeControls({ initialModDetails, onExitEditorMode }: Props) {
  const { t } = useTranslation();

  const [modId, setModId] = useState(initialModDetails.modId);
  const [modWasModified, setModWasModified] = useState(
    initialModDetails.modWasModified
  );
  const [isModCompiled, setIsModCompiled] = useState(
    initialModDetails.compiled
  );
  const [isModDisabled, setIsModDisabled] = useState(
    initialModDetails.compiled && initialModDetails.disabled
  );
  const [isLoggingEnabled, setIsLoggingEnabled] = useState(
    initialModDetails.compiled && initialModDetails.loggingEnabled
  );

  const [compilationFailed, setCompilationFailed] = useState(false);

  useSetEditedModId(
    useCallback((data) => {
      setModId(data.modId);
    }, [])
  );

  // The mod's config can change outside this window, so a details post is the
  // state the host has now, not only the state the sidebar mounted on.
  useSetEditedModDetails(
    useCallback((data) => {
      setModWasModified(data.modWasModified);
      setIsModCompiled(!!data.modDetails);
      setIsModDisabled(!!data.modDetails?.disabled);
      setIsLoggingEnabled(!!data.modDetails?.loggingEnabled);
    }, [])
  );

  const { enableEditedMod } = useEnableEditedMod();
  const setModEnabled = useCallback(
    async (enable: boolean) => {
      const result = await enableEditedMod({ enable });
      if (result.status === 'reply' && result.data.succeeded) {
        setIsModDisabled(!result.data.enabled);
      }
    },
    [enableEditedMod]
  );

  const { enableEditedModLogging } = useEnableEditedModLogging();
  const setModLoggingEnabled = useCallback(
    async (enable: boolean) => {
      const result = await enableEditedModLogging({ enable });
      if (result.status === 'reply' && result.data.succeeded) {
        setIsLoggingEnabled(result.data.enabled);
      }
    },
    [enableEditedModLogging]
  );

  // Both switches stay live while a build runs, so the catch-up below reads them
  // as they stand when the build lands rather than as they were when it started.
  const switchesRef = useRef({ isModDisabled, isLoggingEnabled });
  useEffect(() => {
    switchesRef.current = { isModDisabled, isLoggingEnabled };
  }, [isModDisabled, isLoggingEnabled]);

  const { compileEditedMod, compileEditedModPending } = useCompileEditedMod();

  // A later build leaves the live enable/logging state as is. The first build is
  // always run disabled and without logging so its result can't depend on (and
  // race) the switch state, and the inert mod it produces is brought up to that
  // state here.
  const compileEditedModWithState = useCallback(async () => {
    const wasFirstCompile = !isModCompiled;

    const result = await compileEditedMod(
      wasFirstCompile ? { disabled: true, loggingEnabled: false } : {}
    );
    if (result.status !== 'reply') {
      return;
    }

    if (!result.data.succeeded) {
      setCompilationFailed(true);
      return;
    }

    if (result.data.clearModified) {
      setModWasModified(false);
    }

    setCompilationFailed(false);
    setIsModCompiled(true);

    if (wasFirstCompile) {
      const switches = switchesRef.current;
      // The order is critical: enable logging before enabling the mod, so the
      // mod's execution is captured in the log from its very first call.
      if (switches.isLoggingEnabled) {
        void setModLoggingEnabled(true);
      }
      if (!switches.isModDisabled) {
        void setModEnabled(true);
      }
    }
  }, [compileEditedMod, isModCompiled, setModEnabled, setModLoggingEnabled]);

  useCompileEditedModStart(
    useCallback(() => {
      if (!compileEditedModPending) {
        void compileEditedModWithState();
      }
    }, [compileEditedModWithState, compileEditedModPending])
  );

  const { deleteEditedMod, deleteEditedModPending } = useDeleteEditedMod();
  // The mod is off the machine, so the sidebar is back to what it shows before
  // the first build. The host follows the reply with the details behind that.
  const removeMod = useCallback(async () => {
    const result = await deleteEditedMod({});
    if (result.status === 'reply' && result.data.succeeded) {
      setIsModCompiled(false);
    }
  }, [deleteEditedMod]);

  const { exitEditorMode } = useExitEditorMode();
  const exitEditor = useCallback(
    async (saveToDrafts: boolean) => {
      const result = await exitEditorMode({ saveToDrafts });
      if (result.status === 'reply' && result.data.succeeded) {
        onExitEditorMode?.();
      }
    },
    [exitEditorMode, onExitEditorMode]
  );

  useEditedModWasModified(
    useCallback(() => {
      setModWasModified(true);
      setCompilationFailed(false);
    }, [])
  );

  return (
    <SidebarContainer>
      <Tooltip title={t('sidebar.modId')} placement="bottom">
        <ModIdBox>{modId}</ModIdBox>
      </Tooltip>
      <SwitchesContainer>
        <SwitchesContainerRow>
          <div>{t('sidebar.enableMod')}</div>
          <Tooltip
            title={!isModCompiled && t('sidebar.notCompiled')}
            placement="bottomRight"
          >
            <Switch
              checked={!isModDisabled}
              checkedChildren={!isModCompiled && '✱'}
              onChange={(checked) => void setModEnabled(checked)}
            />
          </Tooltip>
        </SwitchesContainerRow>
        <SwitchesContainerRow>
          <div>{t('sidebar.enableLogging')}</div>
          <Tooltip
            title={!isModCompiled && t('sidebar.notCompiled')}
            placement="bottomRight"
          >
            <Switch
              checked={isLoggingEnabled}
              checkedChildren={!isModCompiled && '✱'}
              onChange={(checked) => void setModLoggingEnabled(checked)}
            />
          </Tooltip>
        </SwitchesContainerRow>
      </SwitchesContainer>
      <ButtonsContainer>
        <CompileButtonBadge
          count={compilationFailed ? '!' : undefined}
          size={compilationFailed ? 'small' : undefined}
          title={
            compilationFailed
              ? (t('sidebar.compilationFailed') as string)
              : undefined
          }
          dot={modWasModified && !compilationFailed}
          status={
            modWasModified && !compilationFailed ? 'default' : undefined
          }
        >
          {compileEditedModPending ? (
            <FullWidthDropdownButton
              type="primary"
              loading
              menu={{
                items: [
                  {
                    key: 'stop',
                    label: t('sidebar.stopCompilation'),
                    onClick: () => stopCompileEditedMod(),
                  },
                ],
              }}
            >
              {t('general.status.compiling')}
            </FullWidthDropdownButton>
          ) : (
            <Button
              type="primary"
              block
              title="Ctrl+B"
              onClick={() => void compileEditedModWithState()}
            >
              {t('sidebar.compile')}
            </Button>
          )}
        </CompileButtonBadge>
        <Button type="primary" block onClick={() => previewEditedMod()}>
          {t('sidebar.preview')}
        </Button>
        <Button type="primary" block onClick={() => showLogOutput()}>
          {t('sidebar.showLogOutput')}
        </Button>
        {isModCompiled && (
          <PopconfirmModal
            placement="bottom"
            title={t('mod.removeConfirm')}
            okText={t('mod.remove')}
            cancelText={t('general.actions.cancel')}
            okButtonProps={{ danger: true }}
            onConfirm={() => void removeMod()}
          >
            <Button
              type="primary"
              danger={true}
              block
              disabled={compileEditedModPending}
              loading={deleteEditedModPending}
            >
              {t('sidebar.remove')}
            </Button>
          </PopconfirmModal>
        )}
        {!initialModDetails.noWindhawkExitButton && (
          <PopconfirmModal
            placement="bottom"
            disabled={!(modWasModified && !isModCompiled) || compileEditedModPending}
            title={t('sidebar.exitConfirmation')}
            okText={t('sidebar.exitButtonOk')}
            cancelText={t('sidebar.exitButtonCancel')}
            onConfirm={() => void exitEditor(false)}
          >
            <Button
              type="primary"
              danger={true}
              block
              disabled={compileEditedModPending}
              onClick={
                modWasModified && !isModCompiled
                  ? undefined
                  : () => void exitEditor(modWasModified)
              }
            >
              {t('sidebar.exit')}
            </Button>
          </PopconfirmModal>
        )}
      </ButtonsContainer>
    </SidebarContainer>
  );
}

export default EditorModeControls;
