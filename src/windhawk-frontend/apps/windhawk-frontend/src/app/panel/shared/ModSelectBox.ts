import styled, { css } from 'styled-components';

// The checkbox's own box, and what it keeps between itself and the mod's name.
const SELECT_BOX_SIZE = 16;
const SELECT_BOX_GAP = 8;

// The room an open box takes, which is what the line it stands in gives up.
export const modSelectBoxRoom = SELECT_BOX_SIZE + SELECT_BOX_GAP;

// What the box looks like once something has called it out. Which conditions do
// that belongs to whoever draws the mod - a card reads its own hover, a table
// reads the row's - so it is a fragment rather than a rule here.
export const modSelectBoxRevealed = css`
  width: ${SELECT_BOX_SIZE}px;
  margin-inline-start: 0;
  opacity: 1;
  pointer-events: auto;
  --mod-select-box-standoff: 0px;
`;

/**
 * What the line the box stands in has to say for the box to travel out from
 * under the mod's own edge: the card's border, the row's cell border.
 *
 * The line reaches back across `padding` - the room between that edge and where
 * the line begins, which is the card's body padding and the cell's - and clips
 * there, so the box comes out from behind the edge rather than out of the middle
 * of the line. The same measurement sets how far past its closed position the
 * checkbox has to stand to be behind that edge: the reach, less the gap the
 * closed box already keeps.
 */
export function modSelectBoxReach(padding: number) {
  return css`
    margin-inline-start: -${padding}px;
    padding-inline-start: ${padding}px;
    overflow: hidden;
    --mod-select-box-standoff: ${padding - SELECT_BOX_GAP}px;
  `;
}

// The box a mod's selection checkbox stands in, at the head of the line the
// mod's name is on: the card's title, the row's name cell.
//
// It is in the flow rather than hung outside the mod. Outside is the 20px the
// page leaves between grid columns, and that gutter is not free: a card with an
// update hangs its ribbon 8px back into it, leaving 12px for a 16px control -
// so a checkbox drawn out there lands on the ribbon of the card beside it. In
// the line there is nothing to collide with.
//
// At rest it is closed to nothing at all: no width, and a negative margin that
// cancels the gap which would otherwise follow it - the card's own, the row's
// cell gap - so a mod nobody is pointing at is laid out to the pixel like one
// that cannot be selected at all. Opening, it takes its room and the name moves
// over, which is the price of standing inside the mod rather than beside it and
// is paid on the hovered mod only.
//
// Where the checkbox travels from is the mod's own edge, which is the line's to
// arrange with modSelectBoxReach: the standoff below stands the control behind
// that edge, and the clip that comes with it is what the slide is bound to.
//
// Transparent rather than hidden, which is the one place this departs from the
// settings form's reorder grip: opacity keeps it in the tab order, so tabbing
// into a mod reaches it and :focus-within opens it the moment focus lands. A
// checkbox has no other way to be checked, where a row has another way to be
// moved. pointer-events is what buys back the grip's reason for hiding
// outright: without it the closed box would be an invisible click target
// sitting over the start of the mod's name.
const ModSelectBox = styled.div`
  display: flex;
  align-items: center;
  justify-content: flex-end;
  flex: none;
  width: 0;
  margin-inline-start: -${SELECT_BOX_GAP}px;
  margin-inline-end: ${SELECT_BOX_GAP}px;
  opacity: 0;
  pointer-events: none;
  transition: width 120ms ease-out, margin-inline-start 120ms ease-out,
    opacity 120ms ease-out;

  // The checkbox keeps its size while the box around it is still opening, and
  // stands off by however far the line's own edge is, so that it starts its
  // travel behind that edge rather than partway along the line.
  > * {
    flex: none;
    position: relative;
    inset-inline-start: calc(-1 * var(--mod-select-box-standoff, 0px));
    transition: inset-inline-start 120ms ease-out;
  }

  &:focus-within {
    ${modSelectBoxRevealed}
  }
`;

export default ModSelectBox;
