import { registerDevToolsInstallPrompt } from '@app/devToolsInstall';
import {
  useCancelInstallDevTools,
  useDevToolsInstalling,
  useDevToolsInstallDownloadProgress,
  useStartInstallDevTools,
} from '@app/webviewIPC';
import { Button, Modal } from 'antd';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { ModalContent, ProgressModalBody } from './progressModalBody';

const Description = styled.div`
  text-align: center;
  font-size: 14px;
`;

type InstallStatus = 'prompt' | 'downloading' | 'installing' | 'failed';

// Mounted once at the app root. It owns the "install development tools" modal and
// registers the opener seam, so a launch entry point that replies uiMissing raises it
// (webviewIPC dispatchDevActionReply -> promptDevToolsInstall). It starts in the
// `prompt` state (explain + offer to install); the install itself renders the
// app-update modal's shared download/install body, driven by its own events.
export function InstallDevToolsModal() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState<InstallStatus>('prompt');
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [errorMessage, setErrorMessage] = useState('');

  // The install-progress events fire on a listener that outlives any single install
  // (this modal is mounted for the app's lifetime, not scoped to a route). The event
  // handlers below therefore read the live open/status through refs, so a stray or
  // out-of-order event is ignored rather than acted on with the values captured when
  // the handler was created.
  const openRef = useRef(open);
  const statusRef = useRef(status);
  useEffect(() => {
    openRef.current = open;
    statusRef.current = status;
  }, [open, status]);

  // Register the opener seam; each open resets to the prompt state.
  useEffect(() => {
    registerDevToolsInstallPrompt(() => {
      setStatus('prompt');
      setDownloadProgress(0);
      setErrorMessage('');
      setOpen(true);
    });
    return () => registerDevToolsInstallPrompt(null);
  }, []);

  const { startInstallDevTools } = useStartInstallDevTools(
    useCallback((data) => {
      // Ignore a reply for an install the user already cancelled (which closed the
      // modal). Success is a no-op regardless: the installer launched and Windhawk
      // will restart.
      if (openRef.current && !data.succeeded) {
        setStatus('failed');
        setErrorMessage(data.error || 'Unknown error');
      }
    }, [])
  );

  useDevToolsInstallDownloadProgress(
    useCallback((data) => {
      // Only advance the bar while a download is actually running; a late event must
      // not drag 'installing'/'failed' back to 'downloading'.
      if (openRef.current && statusRef.current === 'downloading') {
        setDownloadProgress(data.progress);
      }
    }, [])
  );

  useDevToolsInstalling(
    useCallback(() => {
      // Only the download -> install transition; ignore a stray or repeat event.
      if (openRef.current && statusRef.current === 'downloading') {
        setStatus('installing');
      }
    }, [])
  );

  const { cancelInstallDevTools } = useCancelInstallDevTools(
    useCallback((data) => {
      if (data.succeeded) {
        setOpen(false);
      }
      // If cancellation failed, stay in the current state and let the user retry.
    }, [])
  );

  const handleInstall = () => {
    setStatus('downloading');
    setDownloadProgress(0);
    startInstallDevTools({});
  };

  const close = () => setOpen(false);

  // Downloading is the only state the user cannot simply dismiss: closing it means
  // cancelling the in-flight download, so the X is disabled and the footer shows a
  // single danger button that sends the cancel. A click on the mask dismisses the
  // dialog only from the initial prompt, before an install has started.
  const canCancel = status === 'downloading';
  const canClose = !canCancel;
  const maskClosable = status === 'prompt';

  let footer: ReactNode[] | null = null;
  if (status === 'prompt') {
    footer = [
      <Button key="cancel" onClick={close}>
        {t('devTools.install.cancelButton')}
      </Button>,
      <Button key="install" type="primary" onClick={handleInstall}>
        {t('devTools.install.installButton')}
      </Button>,
    ];
  } else if (canCancel) {
    footer = [
      <Button
        key="cancel"
        type="primary"
        danger
        onClick={() => cancelInstallDevTools({})}
      >
        {t('devTools.install.modal.cancel')}
      </Button>,
    ];
  }

  return (
    <Modal
      open={open}
      onCancel={canClose ? close : undefined}
      closable={canClose}
      maskClosable={maskClosable}
      footer={footer}
      title={t('devTools.install.modal.title')}
      width={500}
      centered
    >
      <ModalContent>
        {status === 'prompt' ? (
          <Description>{t('devTools.install.description')}</Description>
        ) : (
          <ProgressModalBody
            failed={status === 'failed'}
            failedTitle={t('devTools.install.modal.failed')}
            errorMessage={errorMessage}
            statusMessage={
              status === 'downloading'
                ? t('devTools.install.modal.downloading')
                : status === 'installing'
                  ? t('devTools.install.modal.installing')
                  : ''
            }
            showProgress={status === 'downloading'}
            downloadProgress={downloadProgress}
            note={
              status === 'installing'
                ? t('devTools.install.modal.installingNote')
                : undefined
            }
          />
        )}
      </ModalContent>
    </Modal>
  );
}
