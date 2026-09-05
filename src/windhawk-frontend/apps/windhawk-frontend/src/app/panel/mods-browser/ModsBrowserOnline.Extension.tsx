import { editMod, forkMod, useGetRepositoryMods } from '@app/webviewIPC';
import { type GetRepositoryModsReplyData } from '@app/webviewIPCMessages';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useParams } from 'react-router-dom';
import { useModOperation } from './modOperation';
import { ModsBrowserOnlineView } from './ModsBrowserOnline.View';
import { useCancelModOperation } from './useCancelModOperation';
import { type InstalledMods } from '../shared/installedMod';
import { useInstalledMods } from './useInstalledMods';

// A listed mod as the catalog describes it, taken from the reply type rather
// than restated: a field the host adds is one this screen already holds. Only
// the repository side - whether a mod is on the machine is the installed
// record's to answer, and that is the half the host goes on sending about.
type CatalogModDetails =
  NonNullable<GetRepositoryModsReplyData['mods']>[string]['repository'];

interface Props {
  ContentWrapper: React.ComponentType<
    React.ComponentPropsWithoutRef<'div'> & { $hidden?: boolean }
  >;
}

export function ModsBrowserOnlineExtension({ ContentWrapper }: Props) {
  const { modId } = useParams<{ modId: string }>();

  const [initialDataPending, setInitialDataPending] = useState(true);
  const [catalogMods, setCatalogMods] = useState<Record<
    string,
    CatalogModDetails
  > | null>(null);

  const {
    installedMods,
    applyInstalledModsListing,
    modWriteMark,
    installMod,
    installModPending,
    compileMod,
    compileModPending,
    enableMod,
    deleteMod,
    updateModRating,
  } = useInstalledMods();

  // IPC: Fetch repository mods. The listing is a catalog with the machine
  // joined into it, and the two are taken apart here: the catalog stands as
  // fetched, the machine's side goes on being answered about for as long as
  // this screen is up - so it is applied at the mark this read was asked for,
  // the way the home screen applies its own.
  const { getRepositoryMods, getRepositoryModsPending } =
    useGetRepositoryMods();

  const refreshRepositoryMods = useCallback(async () => {
    const at = modWriteMark();
    const result = await getRepositoryMods({});
    if (result.status !== 'reply') {
      return;
    }
    const catalog: Record<string, CatalogModDetails> = {};
    const installed: InstalledMods = {};
    for (const [listedModId, mod] of Object.entries(result.data.mods ?? {})) {
      catalog[listedModId] = mod.repository;
      if (mod.installed) {
        installed[listedModId] = mod.installed;
      }
    }
    setCatalogMods(result.data.mods && catalog);
    applyInstalledModsListing(installed, at);
    setInitialDataPending(false);
  }, [getRepositoryMods, modWriteMark, applyInstalledModsListing]);

  useEffect(() => {
    void (async () => {
      await refreshRepositoryMods();
    })();
  }, [refreshRepositoryMods]);

  // What the progress modal is covering: named where the operation is posted,
  // since the reply to it goes to the caller that posted it and says nothing to
  // the modal or to the cancel button in it.
  const { operation: modOperation, track: trackModOperation } =
    useModOperation();

  // IPC: Cancel the install or compile the modal is covering
  const cancelModOperation = useCancelModOperation({
    installModPending,
    compileModPending,
    operation: modOperation,
  });

  // The catalog side of a listed mod. Held rather than written inline: the view
  // filters and sorts the whole catalog behind a memo over these, and an
  // accessor rebuilt each render is a memo that never holds.
  const getModMetadata = useCallback(
    (mod: CatalogModDetails) => mod.metadata,
    []
  );
  const getModMetadataEnglish = useCallback(
    (mod: CatalogModDetails) => mod.metadataEnglish,
    []
  );
  const getModDetails = useCallback((mod: CatalogModDetails) => mod.details, []);

  // Build extension props for ModDetails (only if modId is displayed)
  const modDetailsExtensionProps = useMemo(() => {
    if (!modId || !catalogMods?.[modId]) {
      return undefined;
    }

    return {
      installedModDetails: installedMods?.[modId],
      loadRepositoryData: true,
      actions: {
        installMod: (modSource: string) => {
          trackModOperation({ modId }, installMod({ modId, modSource }));
        },
        updateMod: (modSource: string) => {
          trackModOperation(
            { modId, updating: true },
            installMod({ modId, modSource })
          );
        },
        forkModFromSource: (modSource: string) => forkMod({ modId, modSource }),
        compileMod: () => trackModOperation({ modId }, compileMod({ modId })),
        enableMod: (enable: boolean) => enableMod({ modId, enable }),
        editMod: () => editMod({ modId }),
        forkMod: () => forkMod({ modId }),
        deleteMod: () => deleteMod({ modId }),
        updateModRating: (newRating: number) => updateModRating({ modId, rating: newRating }),
      },
    };
  }, [modId, catalogMods, installedMods, installMod, compileMod, trackModOperation, enableMod, deleteMod, updateModRating]);

  // Waiting on a listing with none in hand: the fetch this screen opens with, and
  // equally the one the failure screen's retry sends. That retry is a round trip
  // over the network, and an error page left up for the whole of it is a press of
  // the button that cannot be told from no press at all; the page coming back
  // after the wait is what says the second attempt failed too.
  const waitingForListing =
    initialDataPending || (!catalogMods && getRepositoryModsPending);

  return (
    <ModsBrowserOnlineView
      ContentWrapper={ContentWrapper}
      repositoryMods={catalogMods}
      initialDataPending={waitingForListing}
      displayedModId={modId}
      getModMetadata={getModMetadata}
      getModMetadataEnglish={getModMetadataEnglish}
      getModDetails={getModDetails}
      installedMods={installedMods}
      showInstallationFilter={true} // Show installation filter in extension mode
      installModPending={installModPending}
      compileModPending={compileModPending}
      modOperation={modOperation}
      onCancelModOperation={cancelModOperation}
      onRetry={refreshRepositoryMods}
      modDetailsExtensionProps={modDetailsExtensionProps}
    />
  );
}
