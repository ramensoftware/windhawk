import {
  editMod,
  forkMod,
  useCompileMod,
  useDeleteMod,
  useEnableMod,
  useGetRepositoryMods,
  useInstallMod,
  useSetNewModConfig,
  useUpdateInstalledModsDetails,
  useUpdateModRating,
} from '@app/webviewIPC';
import {
  type CompileModReplyData,
  type DeleteModReplyData,
  type EnableModReplyData,
  type InstallModReplyData,
  type ModConfig,
  type ModMetadata,
  type RepositoryDetails,
  type UpdateInstalledModsDetailsData,
  type UpdateModRatingReplyData,
} from '@app/webviewIPCMessages';
import { produce } from 'immer';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useParams } from 'react-router-dom';
import { ModsBrowserOnlineView } from './ModsBrowserOnline.View';
import {
  type ModOperationContext,
  useCancelModOperation,
} from './useCancelModOperation';

// Extension mod structure (nested with installed info)
type ExtensionModDetails = {
  repository: {
    metadata: ModMetadata;
    metadataEnglish?: ModMetadata;
    details: RepositoryDetails;
  };
  installed?: {
    metadata: ModMetadata | null;
    config: ModConfig | null;
    userRating?: number;
  };
};

interface Props {
  ContentWrapper: React.ComponentType<
    React.ComponentPropsWithoutRef<'div'> & { $hidden?: boolean }
  >;
}

export function ModsBrowserOnlineExtension({ ContentWrapper }: Props) {
  const { modId } = useParams<{ modId: string }>();

  const [initialDataPending, setInitialDataPending] = useState(true);
  const [repositoryMods, setRepositoryMods] = useState<Record<string, ExtensionModDetails> | null>(null);

  // IPC: Fetch repository mods
  const { getRepositoryMods } = useGetRepositoryMods(
    useCallback((data) => {
      setRepositoryMods(data.mods);
      setInitialDataPending(false);
    }, [])
  );

  useEffect(() => {
    getRepositoryMods({});
  }, [getRepositoryMods]);

  // IPC: Install mod hook
  const { installMod, installModPending, installModContext } = useInstallMod<ModOperationContext>(
    useCallback((data: InstallModReplyData) => {
      const { installedModDetails } = data;
      if (!installedModDetails) {
        return;
      }
      const modId = data.modId;
      setRepositoryMods((prev) =>
        prev &&
        produce(prev, (draft) => {
          draft[modId].installed = installedModDetails;
        })
      );
    }, [])
  );

  // IPC: Compile mod hook
  const { compileMod, compileModPending, compileModContext } = useCompileMod<ModOperationContext>(
    useCallback((data: CompileModReplyData) => {
      const { compiledModDetails } = data;
      if (!compiledModDetails) {
        return;
      }
      const modId = data.modId;
      setRepositoryMods((prev) =>
        prev &&
        produce(prev, (draft) => {
          draft[modId].installed = compiledModDetails;
        })
      );
    }, [])
  );

  // IPC: Cancel the install or compile the modal is covering
  const cancelModOperation = useCancelModOperation({
    installModPending,
    installModContext,
    compileModPending,
    compileModContext,
  });

  // IPC: Enable mod hook
  const { enableMod } = useEnableMod(
    useCallback((data: EnableModReplyData) => {
      if (!data.succeeded) {
        return;
      }
      const modId = data.modId;
      setRepositoryMods((prev) =>
        prev &&
        produce(prev, (draft) => {
          const config = draft[modId].installed?.config;
          if (config) {
            config.disabled = !data.enabled;
          }
        })
      );
    }, [])
  );

  // IPC: Delete mod hook
  const { deleteMod } = useDeleteMod(
    useCallback((data: DeleteModReplyData) => {
      if (!data.succeeded) {
        return;
      }
      const modId = data.modId;
      setRepositoryMods((prev) =>
        prev &&
        produce(prev, (draft) => {
          delete draft[modId].installed;
        })
      );
    }, [])
  );

  // IPC: Update mod rating hook
  const { updateModRating } = useUpdateModRating(
    useCallback((data: UpdateModRatingReplyData) => {
      if (!data.succeeded) {
        return;
      }
      const modId = data.modId;
      setRepositoryMods((prev) =>
        prev &&
        produce(prev, (draft) => {
          const installed = draft[modId].installed;
          if (installed) {
            installed.userRating = data.rating;
          }
        })
      );
    }, [])
  );

  // IPC: Update installed mods details
  useUpdateInstalledModsDetails(
    useCallback((data: UpdateInstalledModsDetailsData) => {
      const installedModsDetails = data.details;
      setRepositoryMods((prev) =>
        prev &&
        produce(prev, (draft) => {
          for (const [modId, updatedDetails] of Object.entries(installedModsDetails)) {
            const details = draft[modId]?.installed;
            if (details) {
              const { userRating } = updatedDetails;
              details.userRating = userRating;
            }
          }
        })
      );
    }, [])
  );

  // IPC: Update mod config (e.g. logging verbosity changed in Advanced tab)
  useSetNewModConfig(
    useCallback(
      (data) => {
        const { modId, config: newConfig } = data;
        setRepositoryMods((prev) =>
          prev &&
          produce(prev, (draft) => {
            const installed = draft[modId]?.installed;
            if (installed?.config) {
              installed.config = {
                ...installed.config,
                ...newConfig,
              };
            }
          })
        );
      },
      []
    )
  );

  // Build extension props for ModDetails (only if modId is displayed)
  const modDetailsExtensionProps = useMemo(() => {
    if (!modId || !repositoryMods?.[modId]) {
      return undefined;
    }

    return {
      installedModDetails: repositoryMods[modId].installed,
      loadRepositoryData: true,
      installMod: (modSource: string) => {
        installMod({ modId, modSource }, { modId });
      },
      updateMod: (modSource: string) => {
        installMod(
          { modId, modSource },
          { modId, updating: true }
        );
      },
      forkModFromSource: (modSource: string) => forkMod({ modId, modSource }),
      compileMod: () => compileMod({ modId }, { modId }),
      enableMod: (enable: boolean) => enableMod({ modId, enable }),
      editMod: () => editMod({ modId }),
      forkMod: () => forkMod({ modId }),
      deleteMod: () => deleteMod({ modId }),
      updateModRating: (newRating: number) => updateModRating({ modId, rating: newRating }),
    };
  }, [modId, repositoryMods, installMod, compileMod, enableMod, deleteMod, updateModRating]);

  return (
    <ModsBrowserOnlineView
      ContentWrapper={ContentWrapper}
      repositoryMods={repositoryMods}
      initialDataPending={initialDataPending}
      displayedModId={modId}
      getModMetadata={(mod) => mod.repository.metadata}
      getModMetadataEnglish={(mod) => mod.repository.metadataEnglish}
      getModDetails={(mod) => mod.repository.details}
      getInstalledDetails={(mod) => mod.installed}
      showInstallationFilter={true} // Show installation filter in extension mode
      installModPending={installModPending}
      compileModPending={compileModPending}
      installModContext={installModContext}
      onCancelModOperation={cancelModOperation}
      onRetry={() => getRepositoryMods({})}
      modDetailsExtensionProps={modDetailsExtensionProps}
    />
  );
}
