import { Button, Modal, Progress, Result } from 'antd';
import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import {
  useCancelUpdate,
  useStartUpdate,
  useUpdateDownloadProgress,
  useUpdateInstalling,
} from '../webviewIPC';

const ModalContent = styled.div`
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 16px 0;
`;

const StatusMessage = styled.div`
  text-align: center;
  font-size: 16px;
`;

const Note = styled.div`
  text-align: center;
  color: var(--vscode-descriptionForeground, #9d9d9d);
  font-size: 14px;
`;

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

  // Start update when modal opens
  useEffect(() => {
    if (props.open) {
      resetState();
      startUpdate({});
    }
  }, [props.open, resetState, startUpdate]);

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
        {status === 'failed' ? (
          <Result
            status="error"
            title={t('about.update.modal.failed')}
            subTitle={errorMessage}
          />
        ) : (
          <>
            <StatusMessage>
              {status === 'downloading' && t('about.update.modal.downloading')}
              {status === 'installing' && t('about.update.modal.installing')}
            </StatusMessage>

            {showProgress && (
              <Progress
                percent={downloadProgress}
                status="active"
              />
            )}

            {status === 'installing' && (
              <Note>{t('about.update.modal.installingNote')}</Note>
            )}
          </>
        )}
      </ModalContent>
    </Modal>
  );
}
