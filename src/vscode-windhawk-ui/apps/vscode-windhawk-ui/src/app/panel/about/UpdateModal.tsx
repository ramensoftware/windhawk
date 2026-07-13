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

type UpdateStatus = 'idle' | 'downloading' | 'installing' | 'failed';

interface Props {
  open: boolean;
  onClose: () => void;
}

export function UpdateModal(props: Props) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<UpdateStatus>('idle');
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [errorMessage, setErrorMessage] = useState('');

  const resetState = useCallback(() => {
    setStatus('idle');
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

  const { startUpdate } = useStartUpdate(useCallback((data) => {
    if (!data.succeeded) {
      setStatus('failed');
      setErrorMessage(data.error || 'Unknown error');
    }

    // At this point, the installer was started successfully.
  }, []));

  // Listen for update progress events
  useUpdateDownloadProgress(
    useCallback((data) => {
      setStatus('downloading');
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
    if (props.open) {
      startUpdate({});
    }
  }, [props.open, startUpdate]);

  const { cancelUpdate } = useCancelUpdate(useCallback((data) => {
    if (data.succeeded) {
      resetAndClose();
    }
    // If cancellation failed, stay in current state and let user try again
  }, [resetAndClose]));

  const canCancel = status === 'downloading';
  const canClose = status === 'installing' || status === 'failed';
  const showProgress = status === 'downloading' || status === 'idle';

  const handleCancel = () => {
    if (canCancel) {
      cancelUpdate({});
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
              type="primary"
              danger
              onClick={handleCancel}
            >
              {t('about.update.modal.cancel')}
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
