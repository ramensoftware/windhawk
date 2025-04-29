import { Checkbox } from 'antd';
import { TooltipPlacement } from 'antd/lib/tooltip';
import React, { JSX, useContext, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { AppUISettingsContext } from '../appUISettings';
import { PopconfirmModal } from '../components/InputWithContextMenu';
import { useUpdateAppSettings } from '../webviewIPC';

const PopconfirmTitleContent = styled.div`
  display: flex;
  flex-direction: column;
  row-gap: 8px;
  max-width: 300px;
`;

interface Props {
  disabled?: boolean;
  popconfirmPlacement?: TooltipPlacement;
  onClick: () => void;
  renderButton: (onClick?: () => void) => JSX.Element;
}

function DevModeAction(props: React.PropsWithChildren<Props>) {
  const { t } = useTranslation();

  const { devModeOptOut, devModeUsedAtLeastOnce } =
    useContext(AppUISettingsContext);

  const [optOutChecked, setOptOutChecked] = useState(false);

  const { updateAppSettings } = useUpdateAppSettings(() => undefined);

  if (devModeOptOut) {
    return null;
  }

  return (
    <PopconfirmModal
      placement={props.popconfirmPlacement}
      disabled={devModeUsedAtLeastOnce || props.disabled}
      title={
        <PopconfirmTitleContent>
          <div>{t('devModeAction.message')}</div>
          <Checkbox
            checked={optOutChecked}
            onChange={(e) => setOptOutChecked(e.target.checked)}
          >
            {t('devModeAction.hideOptionsCheckbox')}
          </Checkbox>
        </PopconfirmTitleContent>
      }
      okText={
        optOutChecked
          ? t('devModeAction.hideOptionsButton')
          : t('devModeAction.beginCodingButton')
      }
      cancelText={t('devModeAction.cancelButton')}
      onConfirm={() => {
        if (optOutChecked) {
          updateAppSettings({
            appSettings: {
              devModeOptOut: true,
            },
          });
        } else {
          updateAppSettings({
            appSettings: {
              devModeUsedAtLeastOnce: true,
            },
          });
          props.onClick();
        }
      }}
      onOpenChange={(open) => open && setOptOutChecked(false)}
    >
      {props.renderButton(
        !devModeUsedAtLeastOnce ? undefined : () => props.onClick()
      )}
    </PopconfirmModal>
  );
}

export default DevModeAction;
