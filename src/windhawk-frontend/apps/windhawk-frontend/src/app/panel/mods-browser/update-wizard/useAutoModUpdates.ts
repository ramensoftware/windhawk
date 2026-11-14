import { useGetRepositoryModSourceData, useInstallMod } from '@app/webviewIPC';
import type { InstalledModDetails } from '@app/webviewIPCMessages';
import { useCallback, useEffect, useRef, useState } from 'react';

/**
 * Installing the mod updates on offer without being asked: the `autoUpdateMods`
 * app setting.
 *
 * It runs over the same list the updates bar counts - every installed mod with an
 * update offered, which already leaves out local mods and the offers the user
 * turned down - and installs them one after another, the way the update wizard
 * does: the repository source is fetched, then handed to `installMod`, so an
 * automatic update is the same install a manual one is.
 *
 * A run starts when the setting is on and an offer is waiting that has not been
 * tried yet. That covers the screen coming up with updates already found, the
 * update check in the background finding new ones while the screen is open (the
 * host pushes them as `updateInstalledModsDetails`), and the setting being turned
 * on.
 *
 * Each offer - a mod at the version it is offered - is tried once per session.
 * One that failed stays on offer, and the updates bar and the wizard are where it
 * is taken up again; trying it again by itself would repeat the failure every
 * time the screen came back, and a new version of the mod is a new offer.
 *
 * A run is the screen's: the screen going away takes the requests in flight with
 * it, and the mods the run had not answered for are tried again when it comes
 * back, as they have no outcome recorded.
 */

export type AutoUpdateCandidate = {
  modId: string;
  name: string;
  // The version on offer, which together with the id is what is tried once.
  offeredVersion: string;
};

export type AutoUpdateRunSummary = {
  updated: AutoUpdateCandidate[];
  failed: AutoUpdateCandidate[];
  // A mod had to be compiled on this machine and the development tools are not
  // installed. The run stops there - the host has already raised the prompt to
  // install them, and every mod after it could need them as well.
  devToolsMissing: boolean;
};

export type AutoUpdateProgress = {
  running: boolean;
  // 1-based position of the mod being updated, and the size of the run.
  current: number;
  total: number;
};

const offerKey = (mod: AutoUpdateCandidate) =>
  `${mod.modId}@${mod.offeredVersion}`;

// The offers tried this session, kept outside the screen so that the home screen
// being left and come back to does not try a failed offer again.
const triedOffers = new Set<string>();

interface Args {
  enabled: boolean;
  mods: AutoUpdateCandidate[];
  // Reported per mod as the run goes, as the wizard does, so the grid loses each
  // badge as it is earned. `profileFieldsKnown` as in `ModUpdateWizard`.
  onModUpdated: (
    modId: string,
    details: InstalledModDetails,
    profileFieldsKnown: boolean
  ) => void;
  onRunFinished: (summary: AutoUpdateRunSummary) => void;
}

export function useAutoModUpdates({
  enabled,
  mods,
  onModUpdated,
  onRunFinished,
}: Args): AutoUpdateProgress {
  const [progress, setProgress] = useState<AutoUpdateProgress>({
    running: false,
    current: 0,
    total: 0,
  });

  // Decided on within the tick a run starts, which state would be too late for:
  // StrictMode runs the effect below twice over the same render.
  const runningRef = useRef(false);
  const mountedRef = useRef(false);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // A run outlives many renders, so it calls the latest callbacks.
  const onModUpdatedRef = useRef(onModUpdated);
  const onRunFinishedRef = useRef(onRunFinished);
  useEffect(() => {
    onModUpdatedRef.current = onModUpdated;
    onRunFinishedRef.current = onRunFinished;
  }, [onModUpdated, onRunFinished]);

  const { getRepositoryModSourceData } = useGetRepositoryModSourceData();
  // Its own install hook rather than the browser's, whose pending state raises
  // the blocking progress modal: an automatic update is a background one.
  const { installMod } = useInstallMod();

  const run = useCallback(
    async (order: AutoUpdateCandidate[]) => {
      const summary: AutoUpdateRunSummary = {
        updated: [],
        failed: [],
        devToolsMissing: false,
      };
      // The mod being worked on, so a request that throws rather than answering
      // still leaves it with an outcome - without one, the end of the run would
      // start the next one on the same mod, and that one would throw too.
      let current: AutoUpdateCandidate | undefined;
      try {
        for (const [index, mod] of order.entries()) {
          if (!mountedRef.current) {
            return;
          }
          current = mod;
          setProgress({ running: true, current: index + 1, total: order.length });

          const sourceResult = await getRepositoryModSourceData({
            modId: mod.modId,
          });
          if (sourceResult.status !== 'reply') {
            // Gone with the screen; the mods left have no outcome, so they are
            // tried again when it comes back.
            return;
          }
          const source = sourceResult.data.data.source;
          if (!source) {
            // An unreachable repository reports a null source.
            triedOffers.add(offerKey(mod));
            summary.failed.push(mod);
            continue;
          }

          const installResult = await installMod({
            modId: mod.modId,
            modSource: source,
          });
          if (installResult.status !== 'reply') {
            return;
          }
          triedOffers.add(offerKey(mod));
          const { installedModDetails, uiMissing, error } = installResult.data;

          if (uiMissing) {
            // Not tried again this session, and neither is the rest of the run:
            // each would raise the same prompt.
            for (const rest of order.slice(index + 1)) {
              triedOffers.add(offerKey(rest));
            }
            summary.failed.push(mod);
            summary.devToolsMissing = true;
            break;
          }
          if (installedModDetails) {
            summary.updated.push(mod);
            onModUpdatedRef.current(mod.modId, installedModDetails, !error);
          } else {
            // Neither host attaches a reason to a failed install; it went to the
            // compiler output.
            summary.failed.push(mod);
          }
        }
      } catch (e) {
        console.error('Automatic mod update failed:', e);
        if (current && !triedOffers.has(offerKey(current))) {
          triedOffers.add(offerKey(current));
          summary.failed.push(current);
        }
      } finally {
        runningRef.current = false;
        if (mountedRef.current) {
          setProgress({ running: false, current: 0, total: 0 });
        }
      }

      if (
        mountedRef.current &&
        (summary.updated.length > 0 || summary.failed.length > 0)
      ) {
        onRunFinishedRef.current(summary);
      }
    },
    [getRepositoryModSourceData, installMod]
  );

  // `progress.running` is a dependency so that the end of a run looks again: an
  // offer that arrived while it was going is taken up by the next one, and so is
  // a mod a run abandoned (which StrictMode's remount does to the first run).
  useEffect(() => {
    if (!enabled || runningRef.current || progress.running) {
      return;
    }
    const pending = mods.filter((mod) => !triedOffers.has(offerKey(mod)));
    if (pending.length === 0) {
      return;
    }
    runningRef.current = true;
    void run(pending);
  }, [enabled, mods, progress.running, run]);

  return progress;
}
