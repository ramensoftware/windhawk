import { useNavigationBlock } from '@app/navigationBlock';
import { Button, Modal } from 'antd';
import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  useCancelUpdate,
  useStartUpdate,
  useUpdateDownloadProgress,
  useUpdateInstalling,
} from '@app/webviewIPC';
import {
  ModalContent,
  ProgressModalBody,
} from '@app/panel/shared/progressModalBody';

// Opening the modal is what starts the update, so `downloading` is where it opens:
// the host's first progress event lands on the first byte of the installer body, and
// a state for the wait before it (name resolution, connect, time to first byte) would
// be one with no cancel and no close in it, held there for as long as a stalled
// network takes to give up. The cancel reaches the host from the first frame - the
// host registers the operation as it takes the request, and a cancel that finds none
// is a harmless no-op.
type UpdateStatus = 'downloading' | 'installing' | 'failed';

interface Props {
  open: boolean;
  onClose: () => void;
}

export function UpdateModal(props: Props) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<UpdateStatus>('downloading');
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [errorMessage, setErrorMessage] = useState('');

  const resetState = useCallback(() => {
    setStatus('downloading');
    setDownloadProgress(0);
    setErrorMessage('');
  }, []);

  const [wasOpen, setWasOpen] = useState(false);

  // Reset local state when the modal transitions to open.
  if (props.open !== wasOpen) {
    setWasOpen(props.open);
    if (props.open) {
      resetState();
    }
  }

  const resetAndClose = useCallback(() => {
    resetState();
    props.onClose();
  }, [props, resetState]);

  const { startUpdate } = useStartUpdate();
  const { cancelUpdate } = useCancelUpdate();

  // Listen for update progress events
  useUpdateDownloadProgress(
    useCallback((data) => {
      setDownloadProgress(data.progress);
    }, [])
  );

  useUpdateInstalling(
    useCallback(() => {
      setStatus('installing');
    }, [])
  );

  // Start the update when the modal opens.
  useEffect(() => {
    if (!props.open) {
      return;
    }

    void (async () => {
      const result = await startUpdate({});
      if (result.status !== 'reply') {
        return;
      }

      // A reply that succeeded means the installer was started, and the host
      // reports the rest through the events above. A failure is this modal's to
      // show: its error is a plain string, so the IPC layer does not surface it.
      if (!result.data.succeeded) {
        setStatus('failed');
        setErrorMessage(result.data.error || 'Unknown error');
      }
    })();
  }, [props.open, startUpdate]);

  const canCancel = status === 'downloading';
  const canClose = status === 'installing' || status === 'failed';
  const showProgress = status === 'downloading';

  // A download the user cannot dismiss is one they cannot walk away from either:
  // leaving this page would take the progress bar and the cancel with it while the
  // host keeps downloading. Once the installer is running the modal says so and
  // lets itself be closed, so nothing is held back from there on.
  useNavigationBlock(props.open && !canClose);

  const handleCancel = async () => {
    if (canCancel) {
      const result = await cancelUpdate({});
      if (result.status === 'reply' && result.data.succeeded) {
        resetAndClose();
      }
      // If cancellation failed, stay in current state and let user try again
    } else if (canClose) {
      resetAndClose();
    }
  };

  return (
    <Modal
      open={props.open}
      onCancel={canClose ? handleCancel : undefined}
      closable={canClose}
      maskClosable={false}
      footer={
        canCancel
          ? [
            <Button
              key="cancel"
              type="primary"
              danger
              onClick={handleCancel}
            >
              {t('general.actions.cancel')}
            </Button>,
          ]
          : null
      }
      title={t('about.update.modal.title')}
      width={500}
      centered
    >
      <ModalContent>
        <ProgressModalBody
          failed={status === 'failed'}
          failedTitle={t('about.update.modal.failed')}
          errorMessage={errorMessage}
          statusMessage={
            status === 'downloading'
              ? t('about.update.modal.downloading')
              : status === 'installing'
                ? t('about.update.modal.installing')
                : ''
          }
          showProgress={showProgress}
          downloadProgress={downloadProgress}
          note={
            status === 'installing'
              ? t('about.update.modal.installingNote')
              : undefined
          }
        />
      </ModalContent>
    </Modal>
  );
}
