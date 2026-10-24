/**
 * The top panels of the source views - their search - drawn outside the view,
 * in a bar the host sticks to the top of what scrolls it, over the text.
 *
 * CodeMirror keeps what it scrolls to, and the edge a drag scrolls at, clear of
 * the panels it draws inside the view, but takes them to reach down from the
 * top of the window, which the region the page scrolls in starts well below.
 * Panels drawn outside the view it does not count, so how far the bar reaches
 * over the view is told to it here.
 */

import type { Extension } from '@codemirror/state';
import { EditorView, panels } from '@codemirror/view';

/**
 * Draws a view's top panels in a part of `bar` of their own, one for each view
 * the bar is over.
 */
export function topPanelsIn(bar: HTMLElement): Extension {
  // Not the bar itself: CodeMirror styles the element it draws panels in as an
  // editor's frame, position included.
  const part = bar.appendChild(document.createElement('div'));
  return [
    panels({ topContainer: part }),
    // All of the bar while it sticks over the text, none of it while it sits
    // above the view.
    EditorView.scrollMargins.of((view) => {
      const { top, bottom } = bar.getBoundingClientRect();
      const textTop = view.scrollDOM.getBoundingClientRect().top;
      return { top: Math.max(0, bottom - Math.max(top, textTop)) };
    }),
  ];
}
