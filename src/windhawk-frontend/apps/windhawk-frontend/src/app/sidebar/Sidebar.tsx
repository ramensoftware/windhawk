import { useCallback, useEffect, useState } from 'react';
import {
  getInitialSidebarParams,
  useSetEditedModDetails,
} from '../webviewIPC';
import EditorModeControls, { type ModDetails } from './EditorModeControls';

function Sidebar() {
  const [modDetails, setModDetails] = useState<ModDetails | null>(
    null
  );

  useEffect(() => {
    getInitialSidebarParams();
  }, []);

  useSetEditedModDetails(
    useCallback((data) => {
      if (!data.modDetails) {
        setModDetails({
          modId: data.modId,
          modWasModified: data.modWasModified,
          noWindhawkExitButton: data.noWindhawkExitButton,
          compiled: false,
        });
      } else {
        setModDetails({
          modId: data.modId,
          modWasModified: data.modWasModified,
          noWindhawkExitButton: data.noWindhawkExitButton,
          compiled: true,
          disabled: data.modDetails.disabled,
          loggingEnabled: data.modDetails.loggingEnabled,
          debugLoggingEnabled: data.modDetails.debugLoggingEnabled,
        });
      }
    }, [])
  );

  const onExitEditorMode = useCallback(() => {
    setModDetails(null);
  }, []);

  if (!modDetails) {
    return null;
  }

  return (
    <EditorModeControls
      key={modDetails.modId}
      initialModDetails={modDetails}
      onExitEditorMode={onExitEditorMode}
    />
  );
}

export default Sidebar;
