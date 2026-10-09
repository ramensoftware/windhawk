// The region an element scrolls in: the nearest ancestor that takes its own
// overflow rather than passing it up. `overlay` is what the panel's content
// region is set to.
export function scrollRegionOf(element: HTMLElement): HTMLElement | null {
  for (let node = element.parentElement; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node);
    if (
      overflowY === 'auto' ||
      overflowY === 'scroll' ||
      overflowY === 'overlay'
    ) {
      return node;
    }
  }
  return null;
}
