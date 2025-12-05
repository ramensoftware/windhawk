import { Alert, Button } from 'antd';
import { useCallback, useContext } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { AppUISettingsContext } from '../appUISettings';
import { PopconfirmModal } from '../components/InputWithContextMenu';
import { useUpdateAppSettings } from '../webviewIPC';

const FullWidthAlert = styled(Alert)`
  padding-inline-start: calc(20px + max(50% - var(--app-max-width) / 2, 0px));
  padding-inline-end: calc(20px + max(50% - var(--app-max-width) / 2, 0px));
`;

const FullWidthAlertContent = styled.div`
  display: flex;
  align-items: center;
  gap: 8px;
`;

function SafeModeIndicator() {
  const { t } = useTranslation();

  const { updateAppSettings } = useUpdateAppSettings(
    useCallback((data) => {
      // Do nothing, we should be restarted soon.
    }, [])
  );

  const { safeMode } = useContext(AppUISettingsContext);

  if (!safeMode) {
    return null;
  }

  return (
    <FullWidthAlert
      message={
        <FullWidthAlertContent>
          <div>{t('safeMode.alert')}</div>
          <div>
            <PopconfirmModal
              title={t('safeMode.offConfirm')}
              okText={t('safeMode.offConfirmOk')}
              cancelText={t('safeMode.offConfirmCancel')}
              onConfirm={() => {
                updateAppSettings({
                  appSettings: {
                    safeMode: false,
                  },
                });
              }}
            >
              <Button ghost>{t('safeMode.offButton')}</Button>
            </PopconfirmModal>
          </div>
        </FullWidthAlertContent>
      }
      banner
    />
  );
}

export default SafeModeIndicator;
