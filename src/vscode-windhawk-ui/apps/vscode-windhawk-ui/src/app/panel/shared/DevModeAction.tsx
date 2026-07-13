import { Checkbox } from 'antd';
import { type TooltipPlacement } from 'antd/lib/tooltip';
import React, {
  type JSX,
  useCallback,
  useContext,
  useEffect,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { AppUISettingsContext } from '@app/appUISettings';
import { PopconfirmModal } from '@app/components/InputWithContextMenu';
import { useUpdateAppSettings } from '@app/webviewIPC';

// Whether the user has started coding with a mod at least once. Stored locally
// so it doesn't round-trip through the extension settings.
const DEV_MODE_USED_AT_LEAST_ONCE_KEY = 'windhawk-devModeUsedAtLeastOnce';

// The editor takes a moment to open after a dev action is triggered. Keep the
// button disabled with a loading indicator for this long so the pending action
// is visible and accidental double clicks are prevented.
const ACTION_LOADING_DURATION_MS = 3000;

const PopconfirmTitleContent = styled.div`
  display: flex;
  flex-direction: column;
  row-gap: 8px;
  max-width: 300px;
`;

interface Props {
  disabled?: boolean;
  popconfirmPlacement?: TooltipPlacement;
  onClick: () => void | Promise<boolean>;
  renderButton: (renderProps: {
    onClick?: () => void;
    loading: boolean;
  }) => JSX.Element;
}

function DevModeAction(props: React.PropsWithChildren<Props>) {
  const { t } = useTranslation();

  const { devModeOptOut } = useContext(AppUISettingsContext);

  const [devModeUsedAtLeastOnce, setDevModeUsedAtLeastOnce] = useState(
    () => localStorage.getItem(DEV_MODE_USED_AT_LEAST_ONCE_KEY) === 'true'
  );

  const [optOutChecked, setOptOutChecked] = useState(false);

  const [loading, setLoading] = useState(false);

  // Clear the loading state once the indicator duration elapses. Keyed on
  // loading so it self-cleans on unmount and if the loading state is reset.
  useEffect(() => {
    if (!loading) {
      return;
    }

    const timeoutId = setTimeout(
      () => setLoading(false),
      ACTION_LOADING_DURATION_MS
    );
    return () => clearTimeout(timeoutId);
  }, [loading]);

  const { updateAppSettings } = useUpdateAppSettings(() => undefined);

  const { onClick } = props;
  const runAction = useCallback(() => {
    setLoading(true);
    // Drop the loading state as soon as the action reports it is not opening the
    // editor; otherwise keep it until the indicator duration elapses.
    Promise.resolve(onClick())
      .then((launching) => {
        if (launching === false) {
          setLoading(false);
        }
      })
      .catch(() => setLoading(false));
  }, [onClick]);

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
          localStorage.setItem(DEV_MODE_USED_AT_LEAST_ONCE_KEY, 'true');
          setDevModeUsedAtLeastOnce(true);
          runAction();
        }
      }}
      onOpenChange={(open) => open && setOptOutChecked(false)}
    >
      {props.renderButton({
        onClick: !devModeUsedAtLeastOnce ? undefined : runAction,
        loading,
      })}
    </PopconfirmModal>
  );
}

export default DevModeAction;
