import { useEffect, useLayoutEffect, useRef } from 'react';
import { useBlocker } from 'react-router-dom';

// What one component wants done about a route change it is not ready for.
type Participant = {
  blocked: boolean;
  confirm?: () => Promise<boolean>;
};

// The live participants, each held as the ref its hook keeps current. A module
// registry rather than a context, so a participant does not have to sit under
// the host to be heard.
const participants = new Set<{ readonly current: Participant }>();

/**
 * Holds back a route change while this component has something the user would
 * otherwise lose: a host operation it is the only progress view of, or an
 * unsaved edit.
 *
 * `confirm` is asked once a navigation is held and decides it - resolving true
 * lets it through, false keeps the user where they are. A participant without
 * one refuses outright, which is what an operation in flight wants: there is
 * nothing to ask, it has to finish.
 *
 * Any number of components can block at once; the host below merges them.
 * Calling `useBlocker` directly does not compose - react-router consults only
 * the blocker registered last, and every other one silently stops working.
 */
export function useNavigationBlock(
  blocked: boolean,
  confirm?: () => Promise<boolean>
) {
  const participant = useRef<Participant>({ blocked, confirm });

  // Both in layout effects, which the commit that changed `blocked` runs before
  // any of its passive effects: a navigation started from one of those - a
  // redirect a screen renders on its way to being blocked - would otherwise
  // consult a registry still describing the render before it.
  useLayoutEffect(() => {
    participant.current = { blocked, confirm };
  }, [blocked, confirm]);

  useLayoutEffect(() => {
    participants.add(participant);
    return () => {
      participants.delete(participant);
    };
  }, []);
}

/**
 * The app's single navigation blocker, mounted in the router's layout. It asks
 * every participant registered through `useNavigationBlock` and resolves the
 * navigation from their answers.
 */
export function NavigationBlockHost() {
  // Who held the navigation the blocker is carrying, captured where it was
  // decided so the resolution below asks exactly those participants.
  const holding = useRef<Participant[]>([]);
  // Set while an answer is being awaited, so a re-render cannot start a second
  // round of the same questions.
  const resolving = useRef(false);

  const blocker = useBlocker(({ currentLocation, nextLocation }) => {
    // A move within the same screen (a search or hash change) takes nothing
    // away, so nobody is asked about it.
    if (currentLocation.pathname === nextLocation.pathname) {
      return false;
    }

    holding.current = Array.from(participants, (entry) => entry.current).filter(
      (entry) => entry.blocked
    );
    return holding.current.length > 0;
  });

  useEffect(() => {
    if (blocker.state !== 'blocked' || resolving.current) {
      return;
    }

    const confirms = holding.current.map((entry) => entry.confirm);

    // One participant with nothing to ask settles it for all of them.
    if (confirms.some((confirm) => !confirm)) {
      blocker.reset();
      return;
    }

    resolving.current = true;
    void (async () => {
      // Asked one at a time rather than all at once: two dialogs over each
      // other read as one prompt appearing twice, and the first refusal already
      // decides the navigation.
      for (const confirm of confirms) {
        if (!(await confirm?.())) {
          resolving.current = false;
          blocker.reset();
          return;
        }
      }
      resolving.current = false;
      blocker.proceed();
    })();
  }, [blocker]);

  return null;
}
