import { Alert, Button } from 'antd';
import { useContext } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { AppUISettingsContext } from '../appUISettings';
import { PopconfirmModal } from '../components/InputWithContextMenu';
import { useUpdateAppSettings } from '../webviewIPC';

const FullWidthAlert = styled(Alert)`
  padding-inline-start: calc(20px + max(50% - var(--whui-max-width) / 2, 0px));
  padding-inline-end: calc(20px + max(50% - var(--whui-max-width) / 2, 0px));
`;

const FullWidthAlertContent = styled.div`
  display: flex;
  align-items: center;
  gap: 8px;
`;

function SafeModeIndicator() {
  const { t } = useTranslation();

  const { updateAppSettings } = useUpdateAppSettings();

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
              cancelText={t('general.actions.cancel')}
              onConfirm={() => {
                // Nothing takes this write's reply: the host restarts the app to
                // apply it, and the banner follows the safe mode the app
                // settings context carries.
                void updateAppSettings({
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
