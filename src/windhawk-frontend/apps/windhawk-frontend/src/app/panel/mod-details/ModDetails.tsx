/// #if WEBSITE
import { ModDetailsWebsite } from './ModDetails.Website';
/// #else
import { ModDetailsExtension } from './ModDetails.Extension';
/// #endif
// The extension variant owns the props both variants are given: it is the one
// that reads all of them. A type import brings no module with it, so the website
// build carries none of its code.
import type { ExtensionProps, RepositoryModDetails } from './ModDetails.Extension';

interface Props {
  modId: string;
  repositoryModDetails?: RepositoryModDetails;
  // Absent for an owner that shows the mod as the whole of its screen, leaving
  // nowhere for the way back to lead.
  goBack?: () => void;

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
