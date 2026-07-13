/// #if WEBSITE
import { ModsBrowserOnlineWebsite } from './ModsBrowserOnline.Website';
/// #else
import { ModsBrowserOnlineExtension } from './ModsBrowserOnline.Extension';
/// #endif

interface Props {
  ContentWrapper: React.ComponentType<
    React.ComponentPropsWithoutRef<'div'> & { $hidden?: boolean }
  >;
}

declare const WEBPACK_IS_WEBSITE: boolean;

function ModsBrowserOnline(props: Props) {
  return WEBPACK_IS_WEBSITE
    ? <ModsBrowserOnlineWebsite {...props} />
    : <ModsBrowserOnlineExtension {...props} />;
}

export default ModsBrowserOnline;
