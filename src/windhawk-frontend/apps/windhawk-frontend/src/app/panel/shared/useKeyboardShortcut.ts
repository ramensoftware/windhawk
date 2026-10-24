import { useEffect, useRef } from 'react';
import { isWindowsPlatform } from '@app/utils/isWindowsPlatform';

// The <input> types that hold no text. A key struck on one of these is no part
// of anything being written, however much the element it came from is called an
// input, and these are where a click commonly leaves focus.
const NON_TEXT_INPUT_TYPES = new Set([
  'button',
  'checkbox',
  'color',
  'file',
  'image',
  'radio',
  'range',
  'reset',
  'submit',
]);

/**
 * Whether the key was struck somewhere text is being written, which is where a
 * plain letter or punctuation key belongs to what is being typed rather than to
 * a shortcut. Combine it into `matches` for any binding that has no modifier to
 * tell it apart from ordinary typing.
 */
export function isTypingTarget(event: KeyboardEvent) {
  const target = event.target as HTMLElement | null;
  if (!target) {
    return false;
  }

  if (target.tagName === 'INPUT') {
    return !NON_TEXT_INPUT_TYPES.has((target as HTMLInputElement).type);
  }

  return target.tagName === 'TEXTAREA' || !!target.isContentEditable;
}

// What the letter check reads off a keystroke, which React's synthetic event
// carries as well as the DOM's.
type LetterKeyEvent = Pick<
  KeyboardEvent,
  'key' | 'keyCode' | 'ctrlKey' | 'altKey' | 'metaKey'
>;

/**
 * Whether the key struck is the one for `letter`, a lowercase Latin letter, in
 * either case. The caller states which modifiers the shortcut takes.
 *
 * On a layout without Latin letters, such as Russian, `key` is the layout's own
 * letter, so with a modifier held the key is named by keyCode instead, as
 * CodeMirror does. Not where the modifiers type a character: Ctrl+Alt is AltGr
 * on Windows, and Alt alone types characters on macOS, so it is trusted only on
 * Windows.
 */
export function isLetterKey(event: LetterKeyEvent, letter: string) {
  const key = event.key.toLowerCase();
  if (key === letter) {
    return true;
  }

  const typesCharacter =
    event.altKey && (event.ctrlKey || (!event.metaKey && !isWindowsPlatform()));
  return (
    (event.ctrlKey || event.altKey || event.metaKey) &&
    !typesCharacter &&
    key.length === 1 &&
    key.charCodeAt(0) > 0x7f &&
    event.keyCode === letter.toUpperCase().charCodeAt(0)
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
