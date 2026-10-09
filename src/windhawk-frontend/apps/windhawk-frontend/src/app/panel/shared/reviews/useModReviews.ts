import useSWR from 'swr';
import { fetchJson, HttpError } from '@app/utils/swrHelpers';
import { type ModReviewsDocument } from './modReviews';
/// #if HAS_MOCKS
import { useMockContext } from '@app/mocking';
/// #endif

declare const WEBPACK_HAS_MOCKS: boolean;

const MOD_REVIEWS_URL_BASE = 'https://mods.windhawk.net/reviews/';

export function modReviewsUrl(modId: string): string {
  return `${MOD_REVIEWS_URL_BASE}${modId}.json`;
}

const emptyDocument = (modId: string): ModReviewsDocument => ({
  modId,
  reviews: [],
});

// A mod with no approved review has no file, so a 404 is the empty document
// rather than a failure.
async function fetchModReviewsDocument(modId: string): Promise<ModReviewsDocument> {
  try {
    return await fetchJson<ModReviewsDocument>(modReviewsUrl(modId));
  } catch (error) {
    if (error instanceof HttpError && error.status === 404) {
      return emptyDocument(modId);
    }
    throw error;
  }
}

export type ModReviewsState = {
  document: ModReviewsDocument | undefined;
  error: unknown;
  isLoading: boolean;
};

// Keyed by the document's URL, so the modal a popover opens reads the entry the
// popover already fetched. The fetcher closes over the mod rather than reading
// it back out of the key.
function useModReviewsDocument(
  modId: string | null,
  fetcher: (modId: string) => Promise<ModReviewsDocument>
): ModReviewsState {
  const { data, error, isLoading } = useSWR(
    modId === null ? null : modReviewsUrl(modId),
    modId === null ? null : () => fetcher(modId)
  );
  return { document: data, error, isLoading };
}

function useModReviewsFromServer(modId: string | null): ModReviewsState {
  return useModReviewsDocument(modId, fetchModReviewsDocument);
}

/// #if HAS_MOCKS
// The document out of the mock registry where there is no host - unless the
// registry says the fetch goes out for real.
function useModReviewsWithMock(modId: string | null): ModReviewsState {
  const { isMockMode, mockData } = useMockContext();
  const fromRegistry = isMockMode && !mockData.commentsOverNetwork;
  return useModReviewsDocument(
    modId,
    fromRegistry
      ? async (id) => mockData.modReviews(id) ?? emptyDocument(id)
      : fetchModReviewsDocument
  );
}
/// #endif

/**
 * A mod's reviews document, fetched once a key is given: null holds the fetch
 * off, which is how a popover waits for its first open.
 */
export const useModReviews: (modId: string | null) => ModReviewsState =
  WEBPACK_HAS_MOCKS ? useModReviewsWithMock : useModReviewsFromServer;
