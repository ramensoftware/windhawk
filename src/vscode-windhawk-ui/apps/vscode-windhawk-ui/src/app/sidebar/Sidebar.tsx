import { useCallback, useEffect, useState } from 'react';
import {
  getInitialSidebarParams,
  useSetEditedModDetails,
} from '../webviewIPC';
import EditorModeControls, { ModDetails } from './EditorModeControls';
import { mockSidebarModDetails } from './mockData';

function Sidebar() {
  const [modDetails, setModDetails] = useState<ModDetails | null>(
    mockSidebarModDetails
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
          compiled: false,
        });
      } else {
        setModDetails({
          modId: data.modId,
          modWasModified: data.modWasModified,
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
