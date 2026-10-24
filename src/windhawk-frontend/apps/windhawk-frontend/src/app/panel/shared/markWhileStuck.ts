/**
 * Keeps `data-stuck` on a sticky bar while the content after it has scrolled
 * under it, so the bar can draw an edge over that content and none while the
 * two only meet. Returns what stops it.
 */
export function markWhileStuck(bar: HTMLElement): () => void {
  let frame = 0;

  const update = () => {
    frame = 0;
    const next = bar.nextElementSibling;
    // Boxes that only meet can overlap by a fraction of a pixel.
    const stuck =
      next !== null &&
      bar.getBoundingClientRect().bottom - next.getBoundingClientRect().top >= 1;
    bar.toggleAttribute('data-stuck', stuck);
  };

  const schedule = () => {
    frame ||= requestAnimationFrame(update);
  };

  // A scroll event does not bubble, but it passes the document on its way to
  // whichever element scrolls. The sizes cover what moves the content without
  // a scroll: the bar growing or shrinking, or what follows it being replaced.
  document.addEventListener('scroll', schedule, { capture: true, passive: true });
  const resizeObserver = new ResizeObserver(schedule);
  resizeObserver.observe(bar);
  if (bar.parentElement) {
    resizeObserver.observe(bar.parentElement);
  }
  schedule();

  return () => {
    document.removeEventListener('scroll', schedule, { capture: true });
    resizeObserver.disconnect();
    cancelAnimationFrame(frame);
  };
}
