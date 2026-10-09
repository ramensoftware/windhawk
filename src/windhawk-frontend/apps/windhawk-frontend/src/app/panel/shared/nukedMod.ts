/**
 * A mod the Windhawk mod catalog took off every machine.
 *
 * The catalog flags an entry's versions with a reason, as `details.nuked`. A
 * machine that sees its installed version flagged stores the reason in the
 * mod's config as `nuked`, and while it is there the engine loads the mod
 * nowhere and the host refuses to enable it. "Flagged" is the catalog's state
 * and "nuked" the machine's.
 */

import {
  type ModConfig,
  type RepositoryDetails,
} from '@app/webviewIPCMessages';

/**
 * The reason a catalog entry flags `version` for, or null where it does not.
 *
 * Read the way the core reads it, so a version the core would not nuke a mod
 * over is not one this blocks installing: `nuked` is a reason for every version,
 * or an object of reasons keyed by version for the versions it names. Every
 * character below U+0020, and U+007F, becomes a space, then the spaces at both
 * ends are trimmed, and what is left is the reason. A value that is neither a
 * string nor an object of strings is one the core acts on neither way.
 */
export function catalogNukedReason(
  details: Pick<RepositoryDetails, 'nuked'> | null | undefined,
  version: string | null | undefined
): string | null {
  const nuked: unknown = details?.nuked;
  let reason: unknown = nuked;
  if (typeof nuked === 'object' && nuked !== null && !Array.isArray(nuked)) {
    const byVersion = nuked as Record<string, unknown>;
    if (!Object.values(byVersion).every((value) => typeof value === 'string')) {
      return null;
    }
    // An inherited member such as `toString` is no string, so it reads as unnamed.
    reason = version != null ? byVersion[version] : undefined;
  }
  if (typeof reason !== 'string') {
    return null;
  }
  // eslint-disable-next-line no-control-regex
  const folded = reason.replace(/[\u0000-\u001f\u007f]/g, ' ').replace(/^ +| +$/g, '');
  return folded || null;
}

/**
 * Whether an installed mod runs: compiled, switched on, and not nuked. A nuked
 * mod runs nowhere, whatever its switch says.
 */
export function modIsRunning(config: ModConfig | null | undefined): boolean {
  return !!config && !config.disabled && !config.nuked;
}
