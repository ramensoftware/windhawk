import { type InitialSettings } from '@app/webviewIPCMessages';
import { ModDetailsSettingsView } from './ModDetailsSettings.View';

interface Props {
  modId: string;
  initialSettings: InitialSettings;
  onCanNavigateAwayChange?: (canNavigateAway: () => Promise<boolean>) => void;
}

export function ModDetailsSettingsWebsite({
  modId,
  initialSettings,
}: Props) {
  // Website mode: read-only preview with empty settings
  // All inputs are disabled and no save/mode toggle buttons are shown
  return (
    <ModDetailsSettingsView
      modId={modId}
      initialSettings={initialSettings}
      modSettingsUI={{}}
      settingsChanged={false}
      readOnly={true}
      onSettingsChange={() => { /* read-only mode */ }}
      onSave={() => { /* read-only mode */ }}
    />
  );
}
