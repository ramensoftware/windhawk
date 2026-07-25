import { type InitialSettings } from '@app/webviewIPCMessages';
/// #if WEBSITE
import { ModDetailsSettingsWebsite } from './ModDetailsSettings.Website';
/// #else
import { ModDetailsSettingsExtension } from './ModDetailsSettings.Extension';
/// #endif

interface Props {
  modId: string;
  initialSettings: InitialSettings;
  readOnly?: boolean;
  onCanNavigateAwayChange?: (canNavigateAway: () => Promise<boolean>) => void;
}

declare const WEBPACK_IS_WEBSITE: boolean;

function ModDetailsSettings(props: Props) {
  return WEBPACK_IS_WEBSITE
    ? <ModDetailsSettingsWebsite {...props} />
    : <ModDetailsSettingsExtension {...props} />;
}

export default ModDetailsSettings;
