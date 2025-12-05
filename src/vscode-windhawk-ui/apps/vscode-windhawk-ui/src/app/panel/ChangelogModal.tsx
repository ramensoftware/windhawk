import { ConfigProvider, Modal, Result, Spin } from 'antd';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import ReactMarkdownCustom from '../components/ReactMarkdownCustom';
import { fetchText } from '../swrHelpers';

const CHANGELOG_URL = 'https://ramensoftware.com/downloads/windhawk_setup.exe?version&changelog';

const ModalContent = styled.div`
  max-height: 60vh;
  overflow-y: auto;
  padding: 16px 0;
`;

const LoadingContainer = styled.div`
  display: flex;
  justify-content: center;
  align-items: center;
  padding: 40px;
`;

interface Props {
  open: boolean;
  onClose: () => void;
}

export function ChangelogModal(props: Props) {
  const { t } = useTranslation();
  const [changelog, setChangelog] = useState('');
  const [loading, setLoading] = useState(false);
  const [hasError, setHasError] = useState(false);
  const fetchStatusRef = useRef<'idle' | 'loading' | 'success' | 'error'>('idle');

  useEffect(() => {
    // Fetch when modal opens if we haven't successfully fetched yet
    // On error, allow retry when modal is reopened (not immediate retry)
    if (props.open && fetchStatusRef.current !== 'success' && fetchStatusRef.current !== 'loading') {
      fetchStatusRef.current = 'loading';
      setLoading(true);
      setHasError(false);

      fetchText(CHANGELOG_URL)
        .then((textWithNull) => {
          const text = textWithNull.split('\0', 2)[1] || '';
          setChangelog(text);
          fetchStatusRef.current = 'success';
          setLoading(false);
        })
        .catch((err) => {
          console.error('Failed to fetch changelog:', err);
          setHasError(true);
          fetchStatusRef.current = 'error';
          setLoading(false);
        });
    }
  }, [props.open]);

  return (
    <Modal
      open={props.open}
      onCancel={props.onClose}
      onOk={props.onClose}
      cancelButtonProps={{ style: { display: 'none' } }}
      okText={t('about.changelog.close')}
      title={t('about.changelog.title')}
      width={700}
      centered
    >
      <ModalContent>
        {loading && (
          <LoadingContainer>
            <Spin />
          </LoadingContainer>
        )}
        {hasError && (
          <Result
            status="error"
            title={t('general.loadingFailedTitle')}
            subTitle={t('general.loadingFailedSubtitle')}
          />
        )}
        {changelog && !loading && !hasError && (
          <ConfigProvider direction="ltr">
            <ReactMarkdownCustom
              markdown={changelog}
              allowHtml
              direction="ltr"
            />
          </ConfigProvider>
        )}
      </ModalContent>
    </Modal>
  );
}
