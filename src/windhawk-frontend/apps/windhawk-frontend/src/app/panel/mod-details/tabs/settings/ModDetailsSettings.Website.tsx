import { type InitialSettings } from '@app/webviewIPCMessages';
import { ModDetailsSettingsView } from './ModDetailsSettings.View';
import { type EditorViewModel } from './useModSettingsEditor';

interface Props {
  modId: string;
  initialSettings: InitialSettings;
  onCanNavigateAwayChange?: (canNavigateAway: () => Promise<boolean>) => void;
}

const noop = () => {
  /* read-only mode */
};

// Website mode: a read-only preview with empty settings. All inputs are
// disabled and there is no save/YAML mode, so a static view model suffices
// (no IPC, no editing state).
const readOnlyViewProps: EditorViewModel = {
  mode: 'ui',
  draft: {},
  canonicalDraft: {},
  canonicalSaved: {},
  arrayMaxIndex: {},
  yamlText: '',
  isDirty: false,
  anySettingModified: false,
  yamlAvailable: false,
  onChangeSetting: noop,
  onAddArrayItem: noop,
  onRemoveArrayItem: noop,
  onRemoveAllArrayItems: noop,
  onMoveArrayItem: noop,
  onResetSetting: noop,
  onSetYamlText: noop,
  onToggleMode: noop,
  onSave: noop,
};

export function ModDetailsSettingsWebsite({
  initialSettings,
}: Props) {
  return (
    <ModDetailsSettingsView
      initialSettings={initialSettings}
      readOnly={true}
      {...readOnlyViewProps}
    />
  );
}
