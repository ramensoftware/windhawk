import { Badge, Button, Dropdown, Switch, Tooltip } from 'antd';
import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { PopconfirmModal } from '../components/InputWithContextMenu';
import {
  previewEditedMod,
  showLogOutput,
  stopCompileEditedMod,
  useCompileEditedMod,
  useCompileEditedModStart,
  useEditedModWasModified,
  useEnableEditedMod,
  useEnableEditedModLogging,
  useExitEditorMode,
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
    border: 1px solid #303030;
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
  background: #444;
  padding: 0 4px;
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

  const { enableEditedMod } = useEnableEditedMod(
    useCallback((data) => {
      if (data.succeeded) {
        setIsModDisabled(!data.enabled);
      }
    }, [])
  );

  const { enableEditedModLogging } = useEnableEditedModLogging(
    useCallback((data) => {
      if (data.succeeded) {
        setIsLoggingEnabled(data.enabled);
      }
    }, [])
  );

  const { compileEditedMod, compileEditedModPending } = useCompileEditedMod(
    useCallback((data) => {
      if (data.succeeded) {
        if (data.clearModified) {
          setModWasModified(false);
        }

        setCompilationFailed(false);
        setIsModCompiled(true);
      } else {
        setCompilationFailed(true);
      }
    }, [])
  );

  const { exitEditorMode } = useExitEditorMode(
    useCallback(
      (data) => {
        if (data.succeeded) {
          onExitEditorMode?.();
        }
      },
      [onExitEditorMode]
    )
  );

  useCompileEditedModStart(
    useCallback(() => {
      if (!compileEditedModPending) {
        compileEditedMod({
          disabled: isModDisabled,
          loggingEnabled: isLoggingEnabled,
        });
      }
    }, [
      compileEditedMod,
      compileEditedModPending,
      isLoggingEnabled,
      isModDisabled,
    ])
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
              onChange={(checked) => enableEditedMod({ enable: checked })}
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
              onChange={(checked) =>
                enableEditedModLogging({ enable: checked })
              }
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
              {t('general.compiling')}
            </FullWidthDropdownButton>
          ) : (
            <Button
              type="primary"
              block
              title="Ctrl+B"
              onClick={() =>
                compileEditedMod({
                  disabled: isModDisabled,
                  loggingEnabled: isLoggingEnabled,
                })
              }
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
        <PopconfirmModal
          placement="bottom"
          disabled={!(modWasModified && !isModCompiled) || compileEditedModPending}
          title={t('sidebar.exitConfirmation')}
          okText={t('sidebar.exitButtonOk')}
          cancelText={t('sidebar.exitButtonCancel')}
          onConfirm={() => exitEditorMode({ saveToDrafts: false })}
        >
          <Button
            type="primary"
            danger={true}
            block
            disabled={compileEditedModPending}
            onClick={
              modWasModified && !isModCompiled
                ? undefined
                : () => exitEditorMode({ saveToDrafts: modWasModified })
            }
          >
            {t('sidebar.exit')}
          </Button>
        </PopconfirmModal>
      </ButtonsContainer>
    </SidebarContainer>
  );
}

export default EditorModeControls;
