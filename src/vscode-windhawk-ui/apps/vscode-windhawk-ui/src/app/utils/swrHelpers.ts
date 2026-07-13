export const fetchText = (input: RequestInfo | URL, init?: RequestInit) =>
  fetch(input, init).then((res) => res.text());

export const fetchJson = <T = unknown>(input: RequestInfo | URL, init?: RequestInit): Promise<T> =>
  fetch(input, init).then((res) => res.json());

const CATALOG_BASE_URL = 'https://mods.windhawk.net/';

export async function fetchCatalogJson<T = unknown>(language: string): Promise<T> {
  // Try language-specific catalog first
  const languageCatalogUrl = `${CATALOG_BASE_URL}catalogs/${language}.json`;
  const response = await fetch(languageCatalogUrl);

  if (response.status === 404) {
    // Fallback to default catalog
    const defaultCatalogUrl = `${CATALOG_BASE_URL}catalog.json`;
    const fallbackResponse = await fetch(defaultCatalogUrl);
    return fallbackResponse.json();
  }

  return response.json();
}
