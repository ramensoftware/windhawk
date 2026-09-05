import { fetchCatalogJson } from '@app/utils/swrHelpers';
import type { ModMetadata, RepositoryDetails } from '@app/webviewIPCMessages';
import { useCallback, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';
import useSWR from 'swr';
import { ModsBrowserOnlineView } from './ModsBrowserOnline.View';

// Website mod structure (flat)
type WebsiteModDetails = {
  metadata: ModMetadata;
  metadataEnglish?: ModMetadata;
  details: RepositoryDetails;
};

interface Props {
  ContentWrapper: React.ComponentType<
    React.ComponentPropsWithoutRef<'div'> & { $hidden?: boolean }
  >;
}

export function ModsBrowserOnlineWebsite({ ContentWrapper }: Props) {
  const { t, i18n } = useTranslation();
  const { modId: displayedModId } = useParams<{ modId: string }>();
  const navigate = useNavigate();

  // Fetch catalog from web with language-specific fallback
  const {
    data: onlineCatalog,
    isLoading,
    mutate: refetchCatalog,
  } = useSWR<{ mods: Record<string, WebsiteModDetails> }>(
    ['catalog', i18n.language],
    () => fetchCatalogJson(i18n.language)
  );

  // Derive state from SWR
  const initialDataPending = isLoading;
  // The error screen is for having no catalog to show, which is what a null here
  // asks for. A fetch that fails over a catalog that arrived - the revalidation
  // SWR runs when the tab is focused again - leaves that catalog in hand, and it
  // stands: the list, its search and the open mod's pane are not worth a failed
  // refresh.
  const repositoryMods = onlineCatalog?.mods ?? null;

  // Held rather than written inline, so the catalog's filter and sort memo holds
  // across renders.
  const getModMetadata = useCallback(
    (mod: WebsiteModDetails) => mod.metadata,
    []
  );
  const getModMetadataEnglish = useCallback(
    (mod: WebsiteModDetails) => mod.metadataEnglish,
    []
  );
  const getModDetails = useCallback((mod: WebsiteModDetails) => mod.details, []);

  // Update document title and redirect if mod not found
  useEffect(() => {
    if (!displayedModId || !repositoryMods) {
      document.title = `${t('website.appHeader.mods')} - Windhawk`;
    } else if (!repositoryMods[displayedModId]) {
      navigate('/mods', { replace: true });
    } else {
      document.title = (repositoryMods[displayedModId].metadata.name || displayedModId) + ' - Windhawk';
    }
  }, [displayedModId, repositoryMods, navigate, t]);

  return (
    <ModsBrowserOnlineView
      ContentWrapper={ContentWrapper}
      repositoryMods={repositoryMods}
      initialDataPending={initialDataPending}
      displayedModId={displayedModId}
      getModMetadata={getModMetadata}
      getModMetadataEnglish={getModMetadataEnglish}
      getModDetails={getModDetails}
      onRetry={() => {
        refetchCatalog();
      }}
    />
  );
}
