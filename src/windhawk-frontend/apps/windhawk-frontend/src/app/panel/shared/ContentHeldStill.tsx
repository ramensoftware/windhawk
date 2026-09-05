import { Component, type ReactNode } from 'react';

// The region an element scrolls in: the nearest ancestor that takes its own
// overflow rather than passing it up. `overlay` is what the panel's content
// region is set to.
function scrollRegionOf(element: HTMLElement) {
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

// Where an element's top edge is within the region it scrolls in. Against the
// region rather than the window, so that a page which moved the region itself
// between one measurement and the next says nothing here.
function topWithin(region: HTMLElement, element: Element) {
  return (
    element.getBoundingClientRect().top -
    region.getBoundingClientRect().top -
    region.clientTop
  );
}

interface Props {
  // Whether the element at the head of the block is being drawn. It has to
  // change in the same render the element does.
  headShown: boolean;
  children: ReactNode;
}

// Where the content stood before the change landed, and the element that stood
// there: null when there is nothing to hold this time round.
type Held = { content: Element; top: number } | null;

/**
 * Holds a block's content still as an element at the head of it comes and goes.
 *
 * A bar that appears above a list pushes the list down, taking the mod the user
 * just pointed at with it. The region the block scrolls in gives up the room
 * instead: it scrolls by as much as the content moved, so what is below the
 * head stays where it was and the room comes out of what is above the block.
 * The same in reverse as the head goes away.
 *
 * The block holds the element that comes and goes at its head, and the content
 * to be held still - the list - as its last element.
 *
 * A class for getSnapshotBeforeUpdate, which is the one thing here that hooks
 * have no answer for: where the content has to be put back to can only be read
 * before the change lands, and every hook runs after it. What moved is then
 * measured rather than worked out from the head's own height, which is what
 * lets this stand alongside the browser's scroll anchoring instead of fighting
 * it: the browser holds a scrolled region's content still on its own account,
 * and where it has, there is nothing here left to do.
 */
class ContentHeldStill extends Component<Props> {
  private block: HTMLDivElement | null = null;

  override getSnapshotBeforeUpdate(previous: Props): Held {
    if (previous.headShown === this.props.headShown) {
      return null;
    }

    const block = this.block;
    const region = block && scrollRegionOf(block);
    const content = block?.lastElementChild;
    return region && content
      ? { content, top: topWithin(region, content) }
      : null;
  }

  override componentDidUpdate(previous: Props, state: unknown, held?: Held) {
    if (!held) {
      return;
    }

    const block = this.block;
    const region = block && scrollRegionOf(block);
    // The content the measurement was taken against has to be the content still
    // there, or the difference is between two different things: a block can lose
    // the list it was holding in the same render its head goes.
    if (!block || !region || block.lastElementChild !== held.content) {
      return;
    }

    const moved = topWithin(region, held.content) - held.top;
    if (moved === 0) {
      return;
    }

    // Room for a head that has just arrived comes out of what is above the
    // block, and there is only so much of it: past the point where the block's
    // top reaches the top of the region, the head sticks there and stands over
    // the very content this is holding still. A block already scrolled past that
    // point has nothing above it left to give - everything on screen is below
    // the head, and all of it moved.
    //
    // Taken instantly, and before the frame is painted: this is a correction to
    // a change nobody has seen yet, not a move to somewhere else.
    const blockTop = topWithin(region, block);
    region.scrollBy({
      top: moved > 0 && blockTop > 0 ? Math.min(moved, blockTop) : moved,
      behavior: 'instant',
    });
  }

  override render() {
    return (
      <div
        ref={(block) => {
          this.block = block;
        }}
      >
        {this.props.children}
      </div>
    );
  }
}

export default ContentHeldStill;
