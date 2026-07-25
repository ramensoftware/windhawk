/**
 * Sanitizes a URL to only allow the http, https, and mailto protocols.
 * Returns undefined if the URL is invalid or uses a disallowed protocol.
 *
 * @param url - The URL to sanitize
 * @returns The sanitized URL or undefined if invalid
 */
export function sanitizeUrl(url: string | undefined): string | undefined {
  if (!url || typeof url !== 'string') {
    return undefined;
  }

  const trimmedUrl = url.trim();
  if (!trimmedUrl) {
    return undefined;
  }

  try {
    const parsed = new URL(trimmedUrl);

    if (parsed.protocol === 'http:' ||
      parsed.protocol === 'https:' ||
      parsed.protocol === 'mailto:') {
      return trimmedUrl;
    }

    return undefined;
  } catch (e) {
    console.warn(`Invalid URL format (${url}):`, e);
    return undefined;
  }
}
