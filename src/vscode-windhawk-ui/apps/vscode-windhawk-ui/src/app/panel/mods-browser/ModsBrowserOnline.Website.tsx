import { fetchCatalogJson } from '@app/utils/swrHelpers';
import type { ModMetadata, RepositoryDetails } from '@app/webviewIPCMessages';
import { useEffect } from 'react';
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
  const { data: onlineCatalog, error: onlineCatalogError, isLoading } = useSWR<{ mods: Record<string, WebsiteModDetails> }>(
    ['catalog', i18n.language],
    () => fetchCatalogJson(i18n.language)
  );

  // Derive state from SWR
  const initialDataPending = isLoading;
  const repositoryMods = onlineCatalogError ? null : (onlineCatalog?.mods ?? null);

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
      getModMetadata={(mod) => mod.metadata}
      getModMetadataEnglish={(mod) => mod.metadataEnglish}
      getModDetails={(mod) => mod.details}
    />
  );
}
