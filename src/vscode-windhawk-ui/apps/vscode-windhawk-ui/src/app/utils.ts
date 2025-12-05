/**
 * Sanitizes a URL to only allow http:// or https:// protocols.
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

    // Only allow http and https protocols
    if (parsed.protocol === 'http:' || parsed.protocol === 'https:') {
      return trimmedUrl;
    }

    return undefined;
  } catch (e) {
    console.warn(`Invalid URL format (${url}):`, e);
    return undefined;
  }
}
