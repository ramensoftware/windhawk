/**
 * In-page links of a rendered markdown document: an href that names an id
 * within the same document, and the element it names.
 */

// rehype-sanitize keeps hast-util-sanitize's default clobberPrefix, which is
// prepended to every id it lets through, so a heading carries the bare slug in
// an unsanitized rendering and the prefixed one in a sanitized rendering. A link
// written in markdown points at the bare slug in both.
const CLOBBER_PREFIX = 'user-content-';

/**
 * Returns the id an href names within the same document, or undefined when the
 * href is not a fragment. The id is empty for a bare '#'.
 */
export function getFragmentId(href: string | undefined): string | undefined {
  const trimmedHref = href?.trim();
  if (trimmedHref === undefined || !trimmedHref.startsWith('#')) {
    return undefined;
  }

  return trimmedHref.slice(1);
}

/**
 * Finds the element a fragment id names. The search is confined to `container`,
 * so two documents rendered on the same page cannot answer for each other's
 * links.
 */
export function findFragmentTarget(
  container: HTMLElement,
  fragmentId: string
): HTMLElement | undefined {
  if (!fragmentId) {
    return undefined;
  }

  // A link destination reaches the DOM percent-encoded, while the ids
  // rehype-slug derives from the heading text keep their characters literal.
  const id = decodeFragmentId(fragmentId);
  const ids = [id, CLOBBER_PREFIX + id];

  return Array.from(container.querySelectorAll<HTMLElement>('[id]')).find(
    element => ids.includes(element.id)
  );
}

function decodeFragmentId(fragmentId: string) {
  try {
    return decodeURIComponent(fragmentId);
  } catch {
    // A malformed escape has no decoding, so the id can only be matched as
    // written.
    return fragmentId;
  }
}
