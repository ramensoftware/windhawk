import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  useGetModSourceData,
  useGetRepositoryModSourceData,
} from '@app/webviewIPC';
import type { ModConfig, ModMetadata, RepositoryDetails } from '@app/webviewIPCMessages';
import { ModDetailsView, type ModSourceData } from './ModDetails.View';
import type { ModStatus } from './ModDetailsHeader';

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

type ViewMode = 'installed' | 'repository' | 'custom';

export function ModDetailsExtension({ modId, repositoryModDetails, goBack, extensionProps }: Props) {
  if (!extensionProps) {
    throw new Error('ModDetailsExtension requires extensionProps');
  }

  // Extract extension data
  const installedModDetails = extensionProps.installedModDetails;
  const loadRepositoryData = extensionProps.loadRepositoryData;

  // Source data for different views (installed, repository latest, custom version)
  const [sourceDataMap, setSourceDataMap] = useState<{
    installed: ModSourceData | null;
    repository: ModSourceData | null;
    custom: ModSourceData | null;
  }>({
    installed: null,
    repository: null,
    custom: null,
  });

  // Version management
  const [customVersionSelection, setCustomVersionSelection] = useState<{
    version: string;
    timestamps: Record<string, number>;
  } | null>(null);
  const [selectedModDetails, setSelectedModDetails] = useState<Exclude<ViewMode, 'custom'> | null>(null);

  // IPC Hook: Fetch installed mod source
  const { getModSourceData } = useGetModSourceData(
    useCallback((data) => {
      if (data.modId === modId) {
        setSourceDataMap(prev => ({ ...prev, installed: data.data }));
      }
    }, [modId])
  );

  useEffect(() => {
    if (installedModDetails?.metadata) {
      getModSourceData({ modId });
    }
  }, [modId, installedModDetails?.metadata, getModSourceData]);

  // IPC Hook: Fetch repository/custom version source
  const { getRepositoryModSourceData } = useGetRepositoryModSourceData(
    useCallback((data) => {
      if (data.modId === modId && data.version === customVersionSelection?.version) {
        if (data.version) {
          setSourceDataMap(prev => ({ ...prev, custom: data.data }));
        } else {
          setSourceDataMap(prev => ({ ...prev, repository: data.data }));
        }
      }
    }, [modId, customVersionSelection?.version])
  );

  // Clear the stale repository source when the viewed mod changes, so the
  // previous mod's source is not shown while the new one is fetched below.
  const [repositorySourceModId, setRepositorySourceModId] = useState<string | null>(null);
  if (repositorySourceModId !== modId) {
    setRepositorySourceModId(modId);
    setSourceDataMap(prev => ({ ...prev, repository: null }));
  }

  useEffect(() => {
    if (repositoryModDetails || loadRepositoryData) {
      getRepositoryModSourceData({ modId });
    }
  }, [getRepositoryModSourceData, loadRepositoryData, modId, repositoryModDetails]);

  // Determine current view mode. The stored selection only applies while both
  // an installed and a repository/custom source are available; otherwise it is
  // ignored so a stale selection never drives the view.
  const effectiveSelection =
    installedModDetails && (repositoryModDetails || loadRepositoryData)
      ? selectedModDetails
      : null;
  const modDetailsToShow: ViewMode = customVersionSelection
    ? 'custom'
    : effectiveSelection || (installedModDetails ? 'installed' : 'repository');

  // Select appropriate source data and metadata based on current view
  const { modMetadata, modSourceData } = useMemo(() => {
    if (modDetailsToShow === 'custom') {
      return {
        modMetadata: sourceDataMap.custom?.metadata || {},
        modSourceData: sourceDataMap.custom,
      };
    } else if (modDetailsToShow === 'installed') {
      return {
        modMetadata: (sourceDataMap.installed ?? installedModDetails)?.metadata || {},
        modSourceData: sourceDataMap.installed,
      };
    } else {
      return {
        modMetadata: (sourceDataMap.repository ?? repositoryModDetails)?.metadata || {},
        modSourceData: sourceDataMap.repository,
      };
    }
  }, [modDetailsToShow, sourceDataMap.custom, sourceDataMap.installed, sourceDataMap.repository, installedModDetails, repositoryModDetails]);

  // The source data for the selected version (custom or repository latest)
  const selectedModSourceData = useMemo(() => {
    return modDetailsToShow === 'custom' ? sourceDataMap.custom : sourceDataMap.repository;
  }, [modDetailsToShow, sourceDataMap.custom, sourceDataMap.repository]);

  const installedVersionIsLatest = useMemo(() => {
    return !!(
      selectedModSourceData?.source &&
      sourceDataMap.installed?.source &&
      selectedModSourceData.source === sourceDataMap.installed.source
    );
  }, [selectedModSourceData, sourceDataMap.installed]);

  const isDowngrade = useMemo(() => {
    if (!customVersionSelection || !installedModDetails?.metadata?.version) {
      return false;
    }
    const selectedTimestamp = customVersionSelection.timestamps[customVersionSelection.version];
    const currentTimestamp = customVersionSelection.timestamps[installedModDetails.metadata.version];
    return selectedTimestamp !== undefined &&
      currentTimestamp !== undefined &&
      selectedTimestamp < currentTimestamp;
  }, [customVersionSelection, installedModDetails]);

  // Determine mod status
  let modStatus: ModStatus = 'not-installed';
  if (modDetailsToShow === 'installed' && installedModDetails) {
    if (!installedModDetails.config) {
      modStatus = 'installed-not-compiled';
    } else if (!installedModDetails.config.disabled) {
      modStatus = 'enabled';
    } else {
      modStatus = 'disabled';
    }
  }

  // Version selector handlers
  const handleViewChange = useCallback((value: Exclude<ViewMode, 'custom'>) => {
    setSelectedModDetails(value);
    setCustomVersionSelection(null);
    setSourceDataMap(prev => ({ ...prev, custom: null }));
  }, []);

  const handleVersionSelect = useCallback((version: string, timestamps: Record<string, number>) => {
    setCustomVersionSelection({ version, timestamps });
    setSourceDataMap(prev => ({ ...prev, custom: null }));
    getRepositoryModSourceData({ modId, version });
  }, [getRepositoryModSourceData, modId]);

  // Compute repository version status for version selector
  const repositoryStatus = useMemo(() => {
    if (!(repositoryModDetails || loadRepositoryData)) {
      return null;
    }
    if (!repositoryModDetails && !sourceDataMap.repository) {
      return { status: 'loading' as const };
    }
    if (!repositoryModDetails && !sourceDataMap.repository?.source) {
      return { status: 'failed' as const };
    }
    // Fall back to repositoryModDetails metadata if source data metadata is not available yet
    const version = (sourceDataMap.repository ?? repositoryModDetails)?.metadata?.version;
    return { status: 'loaded' as const, version };
  }, [repositoryModDetails, loadRepositoryData, sourceDataMap.repository]);

  // Build extension view props (flat structure)
  const extensionViewProps = {
    // Version selector state
    currentView: modDetailsToShow,
    selectedCustomVersion: customVersionSelection?.version ?? null,
    installedVersion: installedModDetails?.metadata?.version,
    repositoryStatus,
    onViewChange: handleViewChange,
    onVersionSelect: handleVersionSelect,

    // Mod state (used by View for tabs AND passed to Header)
    modConfig: (modDetailsToShow === 'installed' && installedModDetails?.config) || undefined,
    modStatus,
    updateAvailable: !!(installedModDetails && (repositoryModDetails || loadRepositoryData)),
    isDowngrade,
    userRating: installedModDetails?.userRating,

    // Action callbacks (passed to Header)
    callbacks: {
      installMod: extensionProps.installMod && selectedModSourceData?.source
        ? () => {
          const source = selectedModSourceData.source;
          if (source) {
            extensionProps.installMod?.(source);
          }
        }
        : undefined,
      updateMod: extensionProps.updateMod && selectedModSourceData?.source
        ? () => {
          const source = selectedModSourceData.source;
          if (source) {
            extensionProps.updateMod?.(source);
          }
        }
        : undefined,
      forkModFromSource: extensionProps.forkModFromSource && selectedModSourceData?.source
        ? () => {
          const source = selectedModSourceData.source;
          if (source) {
            return extensionProps.forkModFromSource?.(source);
          }
        }
        : undefined,
      compileMod: extensionProps.compileMod,
      enableMod: extensionProps.enableMod,
      editMod: extensionProps.editMod,
      forkMod: extensionProps.forkMod,
      deleteMod: extensionProps.deleteMod,
      updateModRating: extensionProps.updateModRating,
    },
  };

  return (
    <ModDetailsView
      modId={modId}
      goBack={goBack}
      modMetadata={modMetadata}
      repositoryDetails={
        (modDetailsToShow === 'repository' && repositoryModDetails?.details) || undefined
      }
      modSourceData={modSourceData}
      installedModSourceData={sourceDataMap.installed}
      selectedModSourceData={selectedModSourceData}
      installedVersionIsLatest={installedVersionIsLatest}
      extensionViewProps={extensionViewProps}
      onRetryLoad={() => {
        if (customVersionSelection) {
          getRepositoryModSourceData({ modId, version: customVersionSelection.version });
        } else if (repositoryModDetails || loadRepositoryData) {
          getRepositoryModSourceData({ modId });
        }
      }}
    />
  );
}
