import { type InitialSettings } from '@app/webviewIPCMessages';
import { Modal } from 'antd';
import { useCallback, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useBlocker } from 'react-router-dom';
import { ModDetailsSettingsView } from './ModDetailsSettings.View';
import { useModSettingsEditor } from './useModSettingsEditor';

interface Props {
  modId: string;
  initialSettings: InitialSettings;
  readOnly?: boolean;
  onCanNavigateAwayChange?: (canNavigateAway: () => Promise<boolean>) => void;
}

export function ModDetailsSettingsExtension({
  modId,
  initialSettings,
  readOnly = false,
  onCanNavigateAwayChange,
}: Props) {
  const { t } = useTranslation();

  const editor = useModSettingsEditor(modId, initialSettings, { readOnly });
  const { isDirty } = editor;

  // Track if a confirmation modal is already open
  const isModalOpenRef = useRef(false);

  // Helper function to show confirmation modal for unsaved changes
  const showUnsavedChangesConfirmation = useCallback((): Promise<boolean> => {
    // Prevent multiple modals from opening
    if (isModalOpenRef.current) {
      return Promise.resolve(false);
    }

    isModalOpenRef.current = true;

    return new Promise((resolve) => {
      Modal.confirm({
        title: t('modDetails.settings.unsavedChangesTitle'),
        content: t('modDetails.settings.unsavedChangesMessage'),
        okText: t('modDetails.settings.unsavedChangesLeave'),
        cancelText: t('modDetails.settings.unsavedChangesStay'),
        onOk: () => {
          isModalOpenRef.current = false;
          resolve(true);
        },
        onCancel: () => {
          isModalOpenRef.current = false;
          resolve(false);
        },
        closable: true,
        maskClosable: true,
      });
    });
  }, [t]);

  // Block navigation when there are unsaved changes
  const blocker = useBlocker(({ currentLocation, nextLocation }) => {
    return isDirty && currentLocation.pathname !== nextLocation.pathname;
  });

  // Show confirmation modal when navigation is blocked
  useEffect(() => {
    if (blocker.state === 'blocked') {
      showUnsavedChangesConfirmation().then((canLeave) => {
        if (canLeave) {
          blocker.proceed();
        } else {
          blocker.reset();
        }
      });
    }
  }, [blocker, showUnsavedChangesConfirmation]);

  // Provide a callback for parent component to check if navigation is allowed
  useEffect(() => {
    const canNavigateAway = (): Promise<boolean> => {
      if (!isDirty) {
        return Promise.resolve(true);
      }

      return showUnsavedChangesConfirmation();
    };

    onCanNavigateAwayChange?.(canNavigateAway);
  }, [isDirty, showUnsavedChangesConfirmation, onCanNavigateAwayChange]);

  if (!editor.ready) {
    return null;
  }

  return (
    <ModDetailsSettingsView
      initialSettings={initialSettings}
      readOnly={readOnly}
      {...editor.viewProps}
    />
  );
}
