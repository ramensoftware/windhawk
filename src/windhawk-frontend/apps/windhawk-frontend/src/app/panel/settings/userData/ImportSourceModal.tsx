import { TextAreaWithContextMenu } from '@app/components/InputWithContextMenu';
import { useInspectUserData } from '@app/webviewIPC';
import { type UserDataManifest } from '@app/webviewIPCMessages';
import { testIdProps } from '@app/utils';
import { Button, Modal, Radio, Space } from 'antd';
import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';

const Body = styled.div`
  display: flex;
  flex-direction: column;
  gap: 16px;
`;

// Where the archive to import comes from: a file the host picks, or text the user
// pastes here.
type Source = 'file' | 'text';

interface Props {
  onClose: () => void;
  // Called with a validated archive and the manifest projected from it, which the
  // import dialog opens over.
  onInspected: (manifest: UserDataManifest, archive: string) => void;
}

// The step ahead of the import dialog: pick where the backup comes from. Either way
// it is the host that validates the archive and projects its manifest, so a bad file
// and bad pasted text fail the same way - with the error the IPC layer surfaces, and
// this dialog left open to retry.
export function ImportSourceModal({ onClose, onInspected }: Props) {
  const { t } = useTranslation();

  const [source, setSource] = useState<Source>('file');
  const [text, setText] = useState('');

  const { inspectUserData, inspectUserDataPending } = useInspectUserData(
    useCallback(
      (data) => {
        // A dismissed Open dialog is a benign no-op; an unreadable file or an invalid
        // archive is auto-surfaced by the IPC layer. Only a valid manifest moves on.
        if (
          data.canceled ||
          !data.succeeded ||
          !data.manifest ||
          data.archive === undefined
        ) {
          return;
        }
        onInspected(data.manifest, data.archive);
      },
      [onInspected]
    )
  );

  const pastedArchive = text.trim();

  const handleContinue = () => {
    // No archive at all leaves the pick to the host: its Open dialog, its read.
    inspectUserData(source === 'text' ? { archive: pastedArchive } : {});
  };

  return (
    <Modal
      open
      title={t('settings.userData.import.title')}
      onCancel={onClose}
      maskClosable={false}
      width={620}
      centered
      wrapProps={testIdProps('import-source-modal')}
      footer={[
        <Button
          key="cancel"
          data-testid="import-source-cancel"
          onClick={onClose}
        >
          {t('general.actions.cancel')}
        </Button>,
        <Button
          key="continue"
          type="primary"
          loading={inspectUserDataPending}
          disabled={source === 'text' && !pastedArchive}
          data-testid="import-source-continue"
          onClick={handleContinue}
        >
          {source === 'text'
            ? t('settings.userData.importSource.continueButton')
            : t('settings.userData.importSource.browseButton')}
        </Button>,
      ]}
    >
      <Body>
        <div>{t('settings.userData.importSource.description')}</div>
        <Radio.Group
          value={source}
          disabled={inspectUserDataPending}
          onChange={(e) => setSource(e.target.value)}
        >
          <Space direction="vertical">
            <Radio value="file" data-testid="import-source-file">
              {t('settings.userData.importSource.fileOption')}
            </Radio>
            <Radio value="text" data-testid="import-source-text">
              {t('settings.userData.importSource.textOption')}
            </Radio>
          </Space>
        </Radio.Group>
        {source === 'text' && (
          <TextAreaWithContextMenu
            rows={10}
            value={text}
            disabled={inspectUserDataPending}
            placeholder={t('settings.userData.importSource.textPlaceholder')}
            data-testid="import-source-textarea"
            onChange={(e) => setText(e.target.value)}
          />
        )}
      </Body>
    </Modal>
  );
}
