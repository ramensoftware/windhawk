import { useLayoutEffect, useRef } from 'react';
import { useLocation } from 'react-router-dom';
import { scrollRegionOf } from './scrollRegion';

// Where the reader left each history entry: how far the region was scrolled,
// and what the block was showing. Keyed by `location.key`, which names an entry
// rather than an address - a back and a forward come to the same one, a link
// followed twice makes two. Kept for the session; a reload starts the history
// over.
const positions = new Map<string, { top: number; content?: string }>();

/**
 * Keeps the region a block scrolls in where the reader left it, per history
 * entry. Moving back or forward to an entry puts the region where it stood when
 * the entry was left; a new entry starts at the top.
 *
 * The block may draw its content after the move - a mod's page reads its source
 * once it is on screen - and the region cannot be put back before there is that
 * much of it to scroll. The block is watched until there is, and the region
 * left alone meanwhile. The reader taking the region in hand ends the wait,
 * which may otherwise never end - a region taller than when the position was
 * taken may never reach it - and the region is theirs from then on.
 *
 * `content` names what the block is showing, for a block that shows more than
 * one thing under an entry: a position taken on one of them is not put back
 * under another. A change of content alone puts nothing back, and ends a wait
 * to put back what was taken on the content before.
 *
 * Returns the ref to attach to the block.
 */
export function useScrollRestoration<T extends HTMLElement>(content?: string) {
  const { key } = useLocation();
  const blockRef = useRef<T>(null);

  // Read when a position is taken rather than closed over: the recorder below
  // stays up through a change of content.
  const contentRef = useRef(content);
  // Ends the wait to put the region back, while there is one.
  const stopWaitingRef = useRef<() => void>();
  useLayoutEffect(() => {
    contentRef.current = content;
    stopWaitingRef.current?.();
  }, [content]);

  // A layout effect rather than a passive one: the recorder has to be on the
  // new entry before the browser reports the scroll the new entry's content
  // made. A region whose content got shorter is clamped, and the clamp is a
  // scroll event dispatched at the next frame - under a recorder still on the
  // entry before, it would be taken for where the reader left that one.
  useLayoutEffect(() => {
    const block = blockRef.current;
    const region = block && scrollRegionOf(block);
    if (!region) {
      return;
    }

    const saved = positions.get(key);
    const top = saved && saved.content === contentRef.current ? saved.top : 0;

    // Puts the region back, unless there is not yet that much of it to scroll:
    // a region put back to where it cannot reach is clamped, and clamped is not
    // where the reader was. Within a pixel: the heights are whole pixels and
    // the position need not be, so a position taken at the foot of the region
    // can stand a fraction past what the heights say it reaches.
    const putBack = () => {
      if (region.scrollHeight - region.clientHeight + 1 < top) {
        return false;
      }
      region.scrollTop = top;
      return true;
    };

    let watcher: ResizeObserver | null = null;
    const stopWaiting = () => {
      watcher?.disconnect();
      watcher = null;
    };
    if (!putBack()) {
      watcher = new ResizeObserver(() => {
        if (putBack()) {
          stopWaiting();
        }
      });
      watcher.observe(block);
    }
    stopWaitingRef.current = stopWaiting;

    // Not while the region is waiting to be put back: where it stands until
    // then is where a page still being drawn stands, and the position the entry
    // holds is worth more than that.
    const record = () => {
      if (!watcher) {
        positions.set(key, {
          top: region.scrollTop,
          content: contentRef.current,
        });
      }
    };
    region.addEventListener('scroll', record, { passive: true });

    // What the reader scrolls with. A press covers the scrollbar and a touch.
    region.addEventListener('wheel', stopWaiting, { passive: true });
    region.addEventListener('pointerdown', stopWaiting);
    region.addEventListener('keydown', stopWaiting);

    return () => {
      region.removeEventListener('scroll', record);
      region.removeEventListener('wheel', stopWaiting);
      region.removeEventListener('pointerdown', stopWaiting);
      region.removeEventListener('keydown', stopWaiting);
      stopWaiting();
      stopWaitingRef.current = undefined;
    };
  }, [key]);

  return blockRef;
}
