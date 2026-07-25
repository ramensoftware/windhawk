/**
 * A response that arrived but reports a failure status. `fetch` resolves for any
 * status the server answers with and rejects only on a network error, so without
 * this the caller would be handed a 404's error page as the document it asked
 * for, and its own error branch would never run.
 */
export class HttpError extends Error {
  readonly status: number;

  constructor(status: number, statusText?: string) {
    super(`Request failed with status ${status}${statusText ? ` ${statusText}` : ''}`);
    this.name = 'HttpError';
    this.status = status;
  }
}

function assertOk(response: Response): Response {
  if (!response.ok) {
    throw new HttpError(response.status, response.statusText);
  }
  return response;
}

export const fetchText = (input: RequestInfo | URL, init?: RequestInit) =>
  fetch(input, init).then((res) => assertOk(res).text());

export const fetchJson = <T = unknown>(input: RequestInfo | URL, init?: RequestInit): Promise<T> =>
  fetch(input, init).then((res) => assertOk(res).json());

const CATALOG_BASE_URL = 'https://mods.windhawk.net/';

export async function fetchCatalogJson<T = unknown>(language: string): Promise<T> {
  // Try language-specific catalog first
  const languageCatalogUrl = `${CATALOG_BASE_URL}catalogs/${language}.json`;
  const response = await fetch(languageCatalogUrl);

  if (response.status === 404) {
    // Fallback to default catalog. Only 404 means "no catalog for this language";
    // any other failure status is reported rather than papered over with the
    // default catalog.
    const defaultCatalogUrl = `${CATALOG_BASE_URL}catalog.json`;
    const fallbackResponse = await fetch(defaultCatalogUrl);
    return assertOk(fallbackResponse).json();
  }

  return assertOk(response).json();
}
