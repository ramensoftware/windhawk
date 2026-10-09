import { useCallback, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import useSWR, { useSWRConfig } from 'swr';
import { useGetRepositoryMods } from '@app/webviewIPC';
import type { ModMetadata, RepositoryDetails } from '@app/webviewIPCMessages';

// As much of the host's listing as the cache takes.
type RepositoryModsListing = Record<
  string,
  {
    repository: {
      metadata: ModMetadata;
      details?: Pick<RepositoryDetails, 'nuked'>;
    };
  }
>;

// What the cache keeps of a mod the host's listing carries.
type ListedMod = {
  name: string | undefined;
  nuked: RepositoryDetails['nuked'];
};

// What the cache holds for a language: each mod the host's listing carries, or
// null where the host had no listing to give.
type RepositoryListing = Record<string, ListedMod> | null;

export type RepositoryModNameState = {
  // The name the repository lists the mod under. Undefined while the listing
  // is on its way, when the host had none, and for a mod it does not list.
  name: string | undefined;
  isLoading: boolean;
};

// Keyed by language the way the website keys its own read of the catalog: the
// host serves the listing in the app's language, and a change of it is a new
// listing.
const listingKey = (language: string) => ['repositoryListing', language];

// The fields read off the cache alone, so the session does not hold the
// listing's others for them.
function cachedListing(mods: RepositoryModsListing | null): RepositoryListing {
  return (
    mods &&
    Object.fromEntries(
      Object.entries(mods).map(([modId, mod]) => [
        modId,
        {
          name: mod.repository.metadata.name,
          nuked: mod.repository.details?.nuked,
        },
      ])
    )
  );
}

/**
 * Hands the cache a listing a screen read from the host, so the screens that
 * read the cache take it from there rather than by a read of their own.
 */
export function useCacheRepositoryListing() {
  const { i18n } = useTranslation();
  const { mutate } = useSWRConfig();
  // The language is read when the listing is handed over, not closed over, so
  // the callback holds still and a screen keying its own read on it does not
  // read again on a language change.
  return useCallback(
    (mods: RepositoryModsListing) => {
      void mutate(listingKey(i18n.language), cachedListing(mods), {
        revalidate: false,
      });
    },
    [i18n, mutate]
  );
}

/**
 * The host's listing: the one a screen handed over, or else, when `read`
 * holds, one read here on first use and kept for the session. App builds only -
 * the website is the repository's own host, and has the catalog in hand.
 */
function useRepositoryListing(read: boolean) {
  const { i18n } = useTranslation();
  const { getRepositoryMods } = useGetRepositoryMods();

  // SWR as the cache and the subscription only: the read below is an effect
  // of its own rather than SWR's fetcher, because the hook it posts through
  // abandons its request on unmount, and the mount-unmount-mount of a strict
  // render would leave SWR deduping the second mount's read against the
  // abandoned first.
  const { data, mutate } = useSWR<RepositoryListing>(
    listingKey(i18n.language),
    null
  );

  useEffect(() => {
    if (!read || data !== undefined) {
      return;
    }
    let current = true;
    void (async () => {
      const result = await getRepositoryMods({});
      // A request the unmount abandoned leaves the entry as it was: the next
      // mount asks again.
      if (!current || result.status !== 'reply') {
        return;
      }
      await mutate(cachedListing(result.data.mods), { revalidate: false });
    })();
    return () => {
      current = false;
    };
  }, [read, data, getRepositoryMods, mutate]);

  return data;
}

/**
 * The name the mod repository lists a mod under, out of the host's listing.
 */
export function useRepositoryModName(modId: string): RepositoryModNameState {
  const listing = useRepositoryListing(true);
  return { name: listing?.[modId]?.name, isLoading: listing === undefined };
}

/**
 * The catalog's `nuked` on a mod, out of the host's listing, read from the host
 * only when `read` holds. Undefined while the listing is on its way, where the
 * host had none, and for a mod it does not list or flag.
 */
export function useRepositoryModNuked(
  modId: string,
  read: boolean
): RepositoryDetails['nuked'] {
  return useRepositoryListing(read)?.[modId]?.nuked;
}
