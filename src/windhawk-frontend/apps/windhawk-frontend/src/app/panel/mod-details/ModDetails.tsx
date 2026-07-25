import type { ModConfig, ModMetadata, RepositoryDetails } from '@app/webviewIPCMessages';
/// #if WEBSITE
import { ModDetailsWebsite } from './ModDetails.Website';
/// #else
import { ModDetailsExtension } from './ModDetails.Extension';
/// #endif

type InstalledModDetails = {
  metadata: ModMetadata | null;
  config: ModConfig | null;
  userRating?: number;
};

type RepositoryModDetails = {
  metadata?: ModMetadata;
  details?: RepositoryDetails;
};

// Extension-only state and callbacks
type ExtensionProps = {
  installedModDetails?: InstalledModDetails;
  loadRepositoryData?: boolean;

  // Action callbacks
  installMod?: (modSource: string) => void;
  updateMod?: (modSource: string) => void;
  forkModFromSource?: (modSource: string) => void;
  compileMod: () => void;
  enableMod: (enable: boolean) => void;
  editMod: () => void;
  forkMod: () => void;
  deleteMod: () => void;
  updateModRating: (newRating: number) => void;
};

interface Props {
  modId: string;
  repositoryModDetails?: RepositoryModDetails;
  goBack: () => void;

  // Extension-specific props (all grouped together)
  extensionProps?: ExtensionProps;
}

declare const WEBPACK_IS_WEBSITE: boolean;

function ModDetails(props: Props) {
  return WEBPACK_IS_WEBSITE
    ? <ModDetailsWebsite {...props} />
    : <ModDetailsExtension {...props} />;
}

export default ModDetails;
