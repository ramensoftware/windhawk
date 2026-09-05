/**
 * An installed mod as a screen holds it.
 *
 * The host's listing entry as it comes, taken from the reply type rather than
 * restated, so a field the host adds is one the screens already hold. It lives
 * here rather than beside any one screen because three of them hold it - both
 * browsers and the editor's preview - and the details screen is handed one.
 *
 * The host sends the terms an update answer is made of and not the answer: the
 * messages a screen follows move one term at a time - a config write turns an
 * offer down, an install takes a version - so an answer held beside them would
 * name an offer that no longer stands wherever a message was missed.
 * `latestVersion` is the one term nothing on this side can work out, which is
 * why it travels; `modHasUpdateOnOffer` reads the three.
 */

import { type GetInstalledModsReplyData } from '@app/webviewIPCMessages';

export type InstalledModEntry =
  GetInstalledModsReplyData['installedMods'][string];

export type InstalledMods = Record<string, InstalledModEntry>;
