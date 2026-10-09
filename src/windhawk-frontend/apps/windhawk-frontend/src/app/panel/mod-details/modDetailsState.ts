/**
 * What the mod details screen knows about a mod, and which actions follow from
 * it.
 *
 * Two facts drive the header, and they are separate: whether the mod is on the
 * machine, and which of its versions the screen is showing. Pairing them in one
 * value is what keeps a mod from reporting itself absent and enabled at once -
 * and what keeps an install, which only a mod that is not there can take, apart
 * from a move to another version, which only a mod that is there can.
 */

import { type InstalledModEntry } from '../shared/installedMod';
import { type UpdateOffer } from '../shared/updateOffer';

// Which version the screen is showing.
export type ShownVersion =
  | { kind: 'installed' }
  | { kind: 'latest' }
  | { kind: 'picked'; version: string };

// A mod that is not on the machine has no installed version to show, so which
// versions can be shown depends on whether there is an installed one.
export type ModDetailsState =
  | { installed: null; shown: Exclude<ShownVersion, { kind: 'installed' }> }
  | { installed: InstalledModEntry; shown: ShownVersion };

// What stands between an action that puts the mod on the machine and its
// running. 'unavailable': the source it would install has not arrived, a wait,
// and the disabled button is the whole of what there is to report. 'nuked': the
// catalog has flagged the version it would install, which no wait ends, and the
// reason it gives is what the button reports.
type InstallBlock = 'unavailable' | 'nuked';

// What the header offers for the mod's update: taking the offer, moving to a
// version asked for by name, or taking a refusal back. Every one of them needs
// an installed mod, so none can be reached for a mod that is not there.
type OfferAction =
  | {
      kind: 'update';
      downgrade: boolean;
      blockedBy?: InstallBlock;
      // The version a pin under this action would name, the other refusal being
      // every version - which needs no name and goes wherever this one does.
      // Null where the action is not the offer's to turn down: an offer already
      // refused is none to refuse again, and over a version asked for by name
      // the action is that version, whatever the offer is doing.
      refusableVersion: string | null;
    }
  | {
      kind: 'allow-updates';
      // The version the refusal is holding off, carried for the sake of what
      // else is shown about it - the source it would bring is worth reading
      // while deciding whether to take the refusal back. Null where there is
      // none to name, which is the refusal standing on its own.
      refusedVersion: string | null;
    };

// What the header offers for the mod itself, beside the offer's own action. One
// of these at a time: a mod that is not on the machine is installed, one that is
// there but was never compiled is compiled, and one that was compiled is turned
// on or off.
type ModAction =
  | { kind: 'install'; blockedBy?: InstallBlock }
  | { kind: 'compile' }
  // A nuked mod is turned on by nothing the user does here: it is shown off,
  // whatever its switch says, and the host refuses to turn it on.
  | { kind: 'enable'; enable: boolean; blockedBy?: 'nuked' };

// What the header offers for the copy on the machine, all of which describe that
// copy and so wait for the view of it. Several stand at once, unlike the single
// action above, so they are a list rather than one of a union.
export type InstalledModAction = 'edit' | 'fork' | 'remove';

// Forking the version on screen into a copy of its own, which is how every
// version but the installed one is forked: the copy on the machine is forked
// from itself, through the actions that describe it.
type ForkFromSourceAction = { blockedBy?: 'unavailable' };

export type HeaderActions = {
  offer: OfferAction | null;
  mod: ModAction | null;
  forkFromSource: ForkFromSourceAction | null;
  // In the order the row reads them.
  installed: InstalledModAction[];
  // Whether the mod can be rated. A rating is of the mod, which has to be on the
  // machine to be rated and to be read from the view of the copy there; a local
  // mod is in no listing to be rated in. Whether that copy was ever compiled is
  // nothing to it.
  rate: boolean;
  // Whether the mod's reviews can be opened for reading and writing, on the
  // same terms as the rating: a review is written about the copy on the
  // machine, and a local mod has no page for one.
  review: boolean;
  // The copy on the machine as the catalog left it: nuked, for the reason
  // stored with it, and restored by taking the offer or only by the catalog
  // clearing it. A flagged version is never offered, so an offer over a nuked
  // mod is of a version seen clean, and installing that one restores the mod -
  // unless the catalog entry this screen holds flags that version. Null for a
  // mod that is not nuked.
  nuked: { reason: string; restoredByUpdate: boolean } | null;
};

export type HeaderActionsInput = {
  state: ModDetailsState;
  offer: UpdateOffer;
  // Whether the version on screen is older than the installed one.
  isDowngrade: boolean;
  // A mod written on this machine, which is nobody's copy of a repository mod:
  // it is edited in place, there is no listing to rate it in, and no repository
  // side an update of it could come from.
  isLocalMod: boolean;
  // What this screen can do at all, from the callbacks its owner passed. What it
  // cannot do it is not handed at all, rather than handed disabled: every state
  // below that shows an action without running it is a wait that ends, and a
  // button its owner wired nothing behind would sit there forever.
  can: { install: boolean; update: boolean; forkFromSource: boolean };
  // Whether the source an action would put on the machine has arrived.
  hasSource: boolean;
  // The reason the catalog entry the screen holds flags the version an install
  // or update here would put on the machine for, or null where it does not flag
  // that version or the screen holds no entry.
  flagged: string | null;
  // Whether that entry flags the version on offer.
  offerFlagged: boolean;
};

/**
 * The actions the header leads with, worked out once from the mod's state.
 *
 * Every button the header draws comes from here: what to do about the update
 * offer, what to do with the mod itself, what to do with the source on screen,
 * what to do with the copy on the machine, and whether it can be rated. Any of
 * them can be absent - a mod showing its installed version with nothing on offer
 * has no offer action and no fork of the source.
 */
export function resolveHeaderActions(input: HeaderActionsInput): HeaderActions {
  const {
    state,
    offer,
    isDowngrade,
    isLocalMod,
    can,
    hasSource,
    flagged,
    offerFlagged,
  } = input;
  const { installed, shown } = state;

  // Every version but the copy on the machine is forked into a copy of its own,
  // and there is a copy to make only once its source is here.
  const forkFromSource: ForkFromSourceAction | null =
    can.forkFromSource && shown.kind !== 'installed'
      ? { blockedBy: hasSource ? undefined : 'unavailable' }
      : null;

  // Ahead of the wait for the source: a flag is not lifted by the source
  // arriving.
  const installBlock: InstallBlock | undefined = flagged
    ? 'nuked'
    : hasSource
      ? undefined
      : 'unavailable';

  if (installed === null) {
    // Nothing to move and nothing to turn back on: the mod is not here, so the
    // one action is putting it here.
    return {
      offer: null,
      mod: can.install ? { kind: 'install', blockedBy: installBlock } : null,
      forkFromSource,
      installed: [],
      rate: false,
      review: false,
      nuked: null,
    };
  }

  const offerAction = ((): OfferAction | null => {
    // A local mod has no repository side, so there is no version to move to, no
    // offer to turn down, and no refusal to take back. The rule is the mod's own
    // rather than the screen's - `modHasUpdateOnOffer` applies it wherever a mod
    // is listed - so it is applied here rather than left to whether the owner
    // happened to ask for a repository side.
    if (isLocalMod) {
      return null;
    }
    // A version the user asked for by name is a move to it whatever the offer is
    // doing - a refusal takes the offer away, not the versions - and a standing
    // offer is one to take from any view, including the installed one.
    if (can.update && (shown.kind === 'picked' || offer.kind === 'standing')) {
      return {
        kind: 'update',
        downgrade: isDowngrade,
        blockedBy: installBlock,
        // Turning the offer down needs one standing to turn down, and a view the
        // offer is what the action means. Over a version asked for by name it is
        // not: the button moves to that version, and what sits under it would
        // refuse a different one the screen is saying nothing about.
        refusableVersion:
          offer.kind === 'standing' && shown.kind !== 'picked'
            ? offer.version
            : null,
      };
    }
    // The way back, in place of the offer the refusal took away. It waits on a
    // view with no action of its own, so it does not stand in for one above.
    if (offer.kind === 'refused') {
      return { kind: 'allow-updates', refusedVersion: offer.version };
    }
    return null;
  })();

  // The mod's own actions describe the copy on the machine, so they wait for the
  // screen to be showing it.
  const showingInstalledVersion = shown.kind === 'installed';

  const modAction = ((): ModAction | null => {
    if (!showingInstalledVersion) {
      return null;
    }
    if (installed.config === null) {
      return { kind: 'compile' };
    }
    if (installed.config.nuked) {
      return { kind: 'enable', enable: true, blockedBy: 'nuked' };
    }
    return { kind: 'enable', enable: !!installed.config.disabled };
  })();

  // Whatever version is on screen: the flag is on the copy on the machine.
  const nukedReason = installed.config?.nuked;

  return {
    offer: offerAction,
    mod: modAction,
    forkFromSource,
    // A local mod is the one copy of itself and is edited where it sits; a
    // repository mod is edited by forking it first.
    installed: !showingInstalledVersion
      ? []
      : isLocalMod
        ? ['edit', 'fork', 'remove']
        : ['fork', 'remove'],
    rate: showingInstalledVersion && !isLocalMod,
    review: showingInstalledVersion && !isLocalMod,
    nuked: nukedReason
      ? {
          reason: nukedReason,
          // A refused offer is one too: the version is named, and allowing
          // updates leads to it.
          restoredByUpdate:
            offer.kind !== 'none' &&
            !!offer.version &&
            !offerFlagged &&
            !isLocalMod,
        }
      : null,
  };
}
