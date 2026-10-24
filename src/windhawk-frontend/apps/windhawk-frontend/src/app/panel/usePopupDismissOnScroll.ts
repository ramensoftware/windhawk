import { useEffect } from 'react';

// Popups are portaled to document.body, outside the content's scroll container.
// antd keeps an open one (tooltip, dropdown, popconfirm, select) aligned to its
// trigger as the container scrolls, so it follows the trigger out of the
// container and over the header, still open over a control that is no longer on
// screen. Dismiss open popups on scroll instead.

const CLICK_POPUP_SELECTOR = '.ant-dropdown, .ant-select-dropdown, .ant-popover';
const TOOLTIP_SELECTOR = '.ant-tooltip';
const ANY_POPUP_SELECTOR = CLICK_POPUP_SELECTOR + ', ' + TOOLTIP_SELECTOR;

// Popup overlays that are currently open (antd toggles a `-hidden` class while
// keeping the overlay mounted).
const OPEN_POPUP_SELECTOR =
  '.ant-dropdown:not(.ant-dropdown-hidden), .ant-select-dropdown:not(.ant-select-dropdown-hidden), .ant-popover:not(.ant-popover-hidden), .ant-tooltip:not(.ant-tooltip-hidden)';

// While this class is on <body>, open popups are hidden instantly (see app.css)
// so the close plays out invisibly, with no re-align slide or fade. Removed once
// the popups have closed, so reopening them later is unaffected.
const DISMISSING_CLASS = 'windhawk-popup-dismissing';

// Wall-clock budget for keeping popups hidden while their close settles, after
// which the dismissing class is removed even if something is still open (e.g. a
// tooltip whose trigger is still hovered when scrolling stops). Time based, not
// frame based, so it does not vary with the display's refresh rate; rAF below is
// only the poll clock. Kept above antd's close animation (~200ms) so the normal,
// state-based exit wins and this is only a backstop.
const RESTORE_DEADLINE_MS = 350;

function usePopupDismissOnScroll() {
  useEffect(() => {
    const root = document.getElementById('root');

    // Popups deliberately portaled into the app content (getPopupContainer set to
    // the trigger's parent) scroll with the content and must be left alone; only
    // body-portaled popups (outside the app root) get stuck and need dismissing.
    // So only body's other children are searched: this runs on every scroll
    // frame, and a search of the root would cost as much as the app is large.
    const openStickyPopups = () =>
      [...document.body.children]
        .filter((container) => container !== root)
        .flatMap((container) => [
          ...(container.matches(OPEN_POPUP_SELECTOR) ? [container] : []),
          ...container.querySelectorAll(OPEN_POPUP_SELECTOR),
        ]);

    let restoreRaf = 0;
    const scheduleRestore = () => {
      cancelAnimationFrame(restoreRaf);
      const deadline = performance.now() + RESTORE_DEADLINE_MS;
      const tick = () => {
        if (openStickyPopups().length === 0 || performance.now() >= deadline) {
          document.body.classList.remove(DISMISSING_CLASS);
        } else {
          restoreRaf = requestAnimationFrame(tick);
        }
      };
      restoreRaf = requestAnimationFrame(tick);
    };

    // Listen for `scroll` rather than `wheel` so every scroll source is covered
    // (wheel, keyboard, scrollbar drag, touch, momentum, programmatic). Scroll
    // events do not bubble, but a capture-phase listener on the document still
    // receives them from whichever descendant element scrolled.
    const handleScroll = (event: Event) => {
      const target = event.target;

      // Don't dismiss when the user is scrolling within a popup itself (e.g. a
      // long, scrollable dropdown menu).
      if (target instanceof Element && target.closest(ANY_POPUP_SELECTOR)) {
        return;
      }

      const open = openStickyPopups();
      if (open.length === 0) {
        return;
      }

      // Hide the open popups (and any that pop up while the class is on) so the
      // close below plays out invisibly instead of sliding to the scrolled
      // trigger and fading.
      document.body.classList.add(DISMISSING_CLASS);

      // Click and contextMenu popups (Dropdown, Select, Popconfirm, the
      // context-menu inputs) close on a document-level `mousedown` whose target
      // is outside the trigger and popup (rc-trigger). Simulate one.
      if (open.some((el) => el.matches(CLICK_POPUP_SELECTOR))) {
        document.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
      }

      // Hover tooltips have no document listener and close only on the trigger's
      // mouse-leave. Tell React the pointer left the currently hovered element by
      // dispatching a native `mouseout` on the deepest one (React derives
      // onMouseLeave from mouseout). This closes a tooltip whose trigger is still
      // under the pointer; if the trigger scrolled out from under it, the browser
      // has already fired its own mouseout.
      if (open.some((el) => el.matches(TOOLTIP_SELECTOR))) {
        const hovered = document.querySelectorAll(':hover');
        hovered[hovered.length - 1]?.dispatchEvent(
          new MouseEvent('mouseout', {
            bubbles: true,
            relatedTarget: document.body,
          })
        );
      }

      scheduleRestore();
    };

    document.addEventListener('scroll', handleScroll, {
      capture: true,
      passive: true,
    });
    return () => {
      document.removeEventListener('scroll', handleScroll, { capture: true });
      cancelAnimationFrame(restoreRaf);
      document.body.classList.remove(DISMISSING_CLASS);
    };
  }, []);
}

export default usePopupDismissOnScroll;
