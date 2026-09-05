import { faArrowRightLong } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import styled from 'styled-components';

// A version pair is a progression, like a number line, so it reads left to right
// whichever way the app is running - and the arrow, being a neutral run between two
// number runs, would be handed back reordered by an RTL paragraph otherwise. The
// isolate keeps that from leaking the other way: the box still sits where the
// surrounding direction puts it.
const Pair = styled.span`
  display: inline-flex;
  align-items: center;
  gap: 6px;
  direction: ltr;
  unicode-bidi: isolate;
  color: var(--whui-text-muted);
`;

// Slightly under the text it sits between, so it separates the two versions
// without competing with them.
const Arrow = styled(FontAwesomeIcon)`
  font-size: 0.85em;
`;

// The version the mod ends up on, which is the half of the pair worth reading. The
// two text tokens are close enough in the light theme that the weight is what
// carries this, not the color.
const To = styled.span`
  color: var(--whui-text-secondary);
  font-weight: 600;
`;

/**
 * The version a mod moves from and the one it moves to. Not a word in it, so it is
 * composed rather than put through a translation key nobody could translate.
 */
export function VersionChange({ from, to }: { from: string; to: string }) {
  return (
    // The pair is on the element as well as in it: a test that reads the versions
    // off the attributes does not have to be rewritten every time this line is
    // dressed differently.
    <Pair data-testid="mod-update-version-change" data-from={from} data-to={to}>
      <span>{from}</span>
      <Arrow icon={faArrowRightLong} />
      <To>{to}</To>
    </Pair>
  );
}

export default VersionChange;
