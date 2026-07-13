import { useGetModSettings, useSetModSettings } from '@app/webviewIPC';
import { type InitialSettings } from '@app/webviewIPCMessages';
import { Modal } from 'antd';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useBlocker } from 'react-router-dom';
import { type ModSettings } from './ModSettingsYaml';
import { ModDetailsSettingsView } from './ModDetailsSettings.View';

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

  // In read-only mode, initialize with empty settings (no IPC fetch needed)
  const [modSettingsUI, setModSettingsUI] = useState<ModSettings | null>(readOnly ? {} : null);
  const [settingsChanged, setSettingsChanged] = useState(false);

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
    return settingsChanged && currentLocation.pathname !== nextLocation.pathname;
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
      if (!settingsChanged) {
        return Promise.resolve(true);
      }

      return showUnsavedChangesConfirmation();
    };

    onCanNavigateAwayChange?.(canNavigateAway);
  }, [settingsChanged, showUnsavedChangesConfirmation, onCanNavigateAwayChange]);

  // IPC hooks
  const { getModSettings } = useGetModSettings(
    useCallback(
      (data) => {
        if (data.modId === modId) {
          setModSettingsUI(data.settings);
        }
      },
      [modId]
    )
  );

  const { setModSettings } = useSetModSettings(
    useCallback(
      (data) => {
        if (data.modId === modId && data.succeeded) {
          setSettingsChanged(false);
        }
      },
      [modId]
    )
  );

  // Fetch settings on mount (only in edit mode)
  useEffect(() => {
    if (!readOnly) {
      getModSettings({ modId });
    }
  }, [getModSettings, modId, readOnly]);

  // Callbacks for View
  const handleSettingsChange = useCallback((newSettings: ModSettings) => {
    setModSettingsUI(newSettings);
    setSettingsChanged(true);
  }, []);

  const handleSave = useCallback(
    (settingsToSave: ModSettings) => {
      setModSettings({
        modId,
        settings: settingsToSave,
      });
    },
    [modId, setModSettings]
  );

  return (
    <ModDetailsSettingsView
      modId={modId}
      initialSettings={initialSettings}
      modSettingsUI={modSettingsUI}
      settingsChanged={settingsChanged}
      readOnly={readOnly}
      onSettingsChange={handleSettingsChange}
      onSave={handleSave}
    />
  );
}
