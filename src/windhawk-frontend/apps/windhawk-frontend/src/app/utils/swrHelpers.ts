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

/**
 * A language tag, which is the only thing that names a catalog file. Anything
 * else is kept out of the URL, where a `/`, a `..` or a `?` would make it ask
 * for a different document than the one the path spells out.
 */
const LANGUAGE_TAG = /^[a-zA-Z]{2,8}(-[a-zA-Z0-9]{1,8})*$/;

const fetchDefaultCatalog = async <T>(): Promise<T> =>
  assertOk(await fetch(`${CATALOG_BASE_URL}catalog.json`)).json();

export async function fetchCatalogJson<T = unknown>(language: string): Promise<T> {
  // A language that cannot name a catalog is served the default one, the same
  // as a language whose catalog does not exist.
  if (!LANGUAGE_TAG.test(language)) {
    return fetchDefaultCatalog();
  }

  // Try language-specific catalog first
  const languageCatalogUrl = `${CATALOG_BASE_URL}catalogs/${language}.json`;
  const response = await fetch(languageCatalogUrl);

  if (response.status === 404) {
    // Only 404 means "no catalog for this language"; any other failure status
    // is reported rather than papered over with the default catalog.
    return fetchDefaultCatalog();
  }

  return assertOk(response).json();
}
