import { useEffect, useRef } from 'react';

/**
 * Whether the key was struck somewhere text is being written, which is where a
 * plain letter or punctuation key belongs to what is being typed rather than to
 * a shortcut. Combine it into `matches` for any binding that has no modifier to
 * tell it apart from ordinary typing.
 */
export function isTypingTarget(event: KeyboardEvent) {
  const target = event.target as HTMLElement | null;
  return (
    target?.tagName === 'INPUT' ||
    target?.tagName === 'TEXTAREA' ||
    !!target?.isContentEditable
  );
}

/**
 * A key combination that acts on the window for as long as it is offered, and
 * takes the keystroke from whatever the browser would otherwise have done with
 * it.
 *
 * `enabled` is what says whether the shortcut is on offer at all: a shortcut for
 * something not on screen is not listened for.
 *
 * Neither callback has to hold its identity across renders. They are read off
 * the latest render when a key arrives, so the listener is hung once and stays
 * for as long as the shortcut is offered, rather than being taken down and put
 * back on every render of the screen that offers it.
 */
export function useKeyboardShortcut(
  enabled: boolean,
  matches: (event: KeyboardEvent) => boolean,
  onMatch: () => void
) {
  const latest = useRef({ matches, onMatch });

  useEffect(() => {
    latest.current = { matches, onMatch };
  });

  useEffect(() => {
    if (!enabled) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (latest.current.matches(event)) {
        event.preventDefault();
        latest.current.onMatch();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [enabled]);
}

export default useKeyboardShortcut;
