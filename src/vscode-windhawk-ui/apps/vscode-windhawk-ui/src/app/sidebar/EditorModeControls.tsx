import { Badge, Button, Modal, Spin, Switch, Tooltip } from 'antd';
import { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled, { css } from 'styled-components';
import { PopconfirmModal } from '../components/InputWithContextMenu';
import {
  previewEditedMod,
  showLogOutput,
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
`;

const ProgressSpin = styled(Spin)`
  display: block;
  margin-left: auto;
  margin-right: auto;
  font-size: 32px;
`;

const ModalWithFocusControl = styled(Modal) <{ $noFocusSteal?: boolean }>`
  ${({ $noFocusSteal }) =>
    $noFocusSteal &&
    css`
      > div[tabindex='0'] {
        display: none;
      }
    `}
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
        setModWasModified(false);
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

  // Without this flag, the modal box steals focus when its shown. It's designed to
  // prevent from further interaction with the currently focused element, but with
  // VSCode it's an issue, since this page is inside an iframe and it steals focus
  // from VSCode. The flag is set if no element inside the iframe has focus, and if so,
  // prevents the focus stealing.
  const modalNoFocusSteal = useMemo(() => {
    if (!compileEditedModPending) {
      return false;
    }

    return !document.activeElement || document.activeElement === document.body;
  }, [compileEditedModPending]);

  return (
    <>
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
          </CompileButtonBadge>
          <Button type="primary" block onClick={() => previewEditedMod()}>
            {t('sidebar.preview')}
          </Button>
          <Button type="primary" block onClick={() => showLogOutput()}>
            {t('sidebar.showLogOutput')}
          </Button>
          <PopconfirmModal
            placement="bottom"
            disabled={!(modWasModified && !isModCompiled)}
            title={t('sidebar.exitConfirmation')}
            okText={t('sidebar.exitButtonOk')}
            cancelText={t('sidebar.exitButtonCancel')}
            onConfirm={() => exitEditorMode({ saveToDrafts: false })}
          >
            <Button
              type="primary"
              danger={true}
              block
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
      <ModalWithFocusControl
        open={compileEditedModPending}
        closable={false}
        footer={null}
        $noFocusSteal={modalNoFocusSteal}
      >
        <ProgressSpin size="large" tip={t('general.compiling')} />
      </ModalWithFocusControl>
    </>
  );
}

export default EditorModeControls;
