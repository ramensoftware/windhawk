/**
 * What counts as an update being offered: the version the repository holds,
 * against the installed one and the suppression stored on the mod.
 *
 * The host answers this per installed mod and sends the answer along -
 * `updateAvailable`, over the `latestVersion` it was reached on. The answer does
 * not keep beside the fields it is drawn from, which the messages a screen
 * follows move: a config write turns an offer down, an install takes a version.
 * So `latestVersion` is what a screen holds - nothing on this side can work that
 * out - and the answer is reached from it here, by the host's own rule, so the
 * same mod reads the same way wherever it is shown.
 */

import { isLocalModId } from '@app/utils';
import {
  parseSuppression,
  suppressesUpdateOffer,
  type ModConfig,
  type ModMetadata,
} from '@app/webviewIPCMessages';

// Where the repository stands against the machine. The three cases are apart
// because a screen acts differently on each: an offer names the version it would
// bring, being up to date rules an offer out, and a side that has not arrived
// rules nothing out - a mod that is up to date would otherwise read as one whose
// offer is still on its way.
export type RepositoryComparison =
  | { readonly kind: 'unknown' }
  | { readonly kind: 'upToDate' }
  | { readonly kind: 'offered'; readonly version: string };

// The host's test: the repository holding a version other than the installed one
// is an offer of it. An installed version the host cannot read is the empty
// string there, which differs from every version the repository names, so a mod
// whose metadata is missing is offered what the repository holds.
export function compareToRepository(
  installedVersion: string | null | undefined,
  repositoryVersion: string | null | undefined
): RepositoryComparison {
  if (!repositoryVersion) {
    return { kind: 'unknown' };
  }
  return repositoryVersion === installedVersion
    ? { kind: 'upToDate' }
    : { kind: 'offered', version: repositoryVersion };
}

// What a mod's update offer amounts to, with the suppression the user stored
// already matched against the version being offered: a pin the repository has
// moved past refuses nothing, and is no refusal here.
export type UpdateOffer =
  // Nothing newer to take, or no repository side to learn of one from.
  | { readonly kind: 'none' }
  // The version an update would bring. An offer names one: it is reached from
  // the version the repository holds, so there is no offer before there is a
  // version to have reached it from.
  | { readonly kind: 'standing'; readonly version: string }
  // A stored suppression that covers the offer, leaving the way back to act on.
  | {
      readonly kind: 'refused';
      // The version the suppression is holding off, which is what a reader
      // weighing whether to lift it is deciding about. Null where there is
      // nothing behind the refusal to name: the repository holds the installed
      // version, or has not been read - a refusal outlives the side it was
      // stored against.
      readonly version: string | null;
    };

// The three terms an offer is reached from, which is the host's own
// `updateAvailable` rule: the comparison above, and the suppression stored
// against the mod. Every screen that says anything about a mod's updates - a
// badge, a count, a filter, the details header - reads this, so the same mod
// reads the same way wherever it is shown.
export function resolveUpdateOffer({
  installedVersion,
  repositoryVersion,
  storedSuppression,
}: {
  installedVersion: string | null | undefined;
  repositoryVersion: string | null | undefined;
  storedSuppression: string | null | undefined;
}): UpdateOffer {
  const stored = storedSuppression ?? '';
  const comparison = compareToRepository(installedVersion, repositoryVersion);
  if (
    comparison.kind === 'offered' &&
    !suppressesUpdateOffer(stored, comparison.version)
  ) {
    return { kind: 'standing', version: comparison.version };
  }
  // A refusal outlives the repository side: the host stops naming an update for
  // a mod that turned them off, which is the state the way back is for.
  return parseSuppression(stored)
    ? {
        kind: 'refused',
        version: comparison.kind === 'offered' ? comparison.version : null,
      }
    : { kind: 'none' };
}

// The rule above over an installed mod, which holds all three of its terms, and
// answering only whether an update is waiting - which is what a screen showing a
// badge, a count or a filter has to know.
//
// A local mod is never offered one: it is nobody's copy of a repository mod, so
// there is no source an update of it could come from. The host reports no
// version for one, which would answer this on its own - but a cached version
// that outlived a mod becoming local would otherwise produce an offer nothing
// can act on, and every caller here would need the same guard of its own.
export function modHasUpdateOnOffer(
  modId: string,
  mod: {
    metadata: ModMetadata | null;
    config: ModConfig | null;
    latestVersion: string | null;
  }
): boolean {
  return (
    !isLocalModId(modId) &&
    resolveUpdateOffer({
      installedVersion: mod.metadata?.version,
      repositoryVersion: mod.latestVersion,
      storedSuppression: mod.config?.updatesDisabledForVersion,
    }).kind === 'standing'
  );
}
