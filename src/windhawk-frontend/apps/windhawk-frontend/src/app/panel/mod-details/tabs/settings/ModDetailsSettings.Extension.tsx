import { useNavigationBlock } from '@app/navigationBlock';
import { useUnsavedChangesPrompt } from '@app/panel/shared/useUnsavedChangesPrompt';
import { type InitialSettings } from '@app/webviewIPCMessages';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
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
  const { isDirty, isSaving } = editor;

  const showUnsavedChangesConfirmation = useUnsavedChangesPrompt({
    title: t('modDetails.settings.unsavedChangesTitle') as string,
    message: t('modDetails.settings.unsavedChangesMessage') as string,
    leave: t('modDetails.settings.unsavedChangesLeave') as string,
    stay: t('modDetails.settings.unsavedChangesStay') as string,
  });

  // A save in flight is held rather than asked about. The changes are still dirty
  // until the host answers, so the question would be whether to discard changes
  // the host is already applying - and either answer to it would be a lie. The
  // hold lasts until the reply, which settles them one way or the other.
  useNavigationBlock(isSaving);

  // Block navigation when there are unsaved changes, asking what to do with them
  useNavigationBlock(isDirty && !isSaving, showUnsavedChangesConfirmation);

  // Provide a callback for parent component to check if navigation is allowed
  useEffect(() => {
    const canNavigateAway = (): Promise<boolean> => {
      if (isSaving) {
        // Held for the same reason, so a tab switch cannot get around it.
        return Promise.resolve(false);
      }

      if (!isDirty) {
        return Promise.resolve(true);
      }

      return showUnsavedChangesConfirmation();
    };

    onCanNavigateAwayChange?.(canNavigateAway);
  }, [isDirty, isSaving, showUnsavedChangesConfirmation, onCanNavigateAwayChange]);

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
