import {
  useCompileMod,
  useDeleteMod,
  useEnableMod,
  useGetInstalledMods,
  useGetModConfig,
  useInstallMod,
  useSetNewModConfig,
  useUpdateInstalledModsDetails,
  useUpdateModRating,
} from '@app/webviewIPC';
import {
  type CompileModData,
  type DeleteModData,
  type EnableModData,
  type InstallModData,
  type InstalledModDetails,
  type UpdateModRatingData,
} from '@app/webviewIPCMessages';
import { produce } from 'immer';
import { useCallback } from 'react';
import { type InstalledModEntry } from '../shared/installedMod';
import { useResync } from '../shared/useResync';
import { useVersionedStore } from '../shared/useVersionedStore';

/**
 * The mods on the machine, and the host messages that move one without being
 * asked to: a config written from anywhere, an update check coming back.
 *
 * Every screen that shows an installed mod follows these, whether or not it can
 * act on one - the editor's preview reads the mod without a single action wired
 * - so they are held apart from the actions in `useInstalledMods` below.
 *
 * The first read is the screen's to choose: the plain listing, read here with
 * `readListingOnMount`, or a listing joined into something else and handed over
 * with `applyInstalledModsListing`, at the mark taken before asking. That is the
 * only way in, so a screen cannot fill the set by a route that forgets the mark.
 * Reading the plain listing again when the window comes back is this hook's
 * own.
 */
export function useInstalledModsState({
  readListingOnMount = false,
}: { readListingOnMount?: boolean } = {}) {
  const {
    items: installedMods,
    setItems: setInstalledMods,
    held: heldMod,
    mark: modWriteMark,
    writtenSince: modWrittenSince,
    applyWrite,
    applyRead: applyInstalledModsListing,
  } = useVersionedStore<InstalledModEntry>();

  // The mark is taken before asking, so a mod this window enables or removes
  // while the listing is out keeps what the command left it.
  const { getInstalledMods } = useGetInstalledMods();
  const refreshInstalledMods = useCallback(async () => {
    const at = modWriteMark();
    const result = await getInstalledMods({});
    if (result.status === 'reply') {
      applyInstalledModsListing(result.data.installedMods, at);
    }
  }, [getInstalledMods, modWriteMark, applyInstalledModsListing]);

  // A mod can be installed, enabled, configured, switched to another user
  // configuration or removed outside this window.
  useResync(refreshInstalledMods, { onMount: readListingOnMount });

  // What an install or a recompile leaves on the machine: the mod as the host
  // lists it now, which is the whole entry - a mod that was not on the machine
  // needs no values invented for it.
  //
  // `profileFieldsKnown` is false where the reply came with an error beside its
  // details: the operation landed, so what it did is reported, but the host
  // could not read the mod back afterwards and the two profile-held fields are
  // its stand-ins for "nothing known". Adopting those would replace a rating
  // that was really there with an unrated 0, and nothing pushes the rating again
  // (the profile write behind an install is the host's own, so no update event
  // follows) - it would stand until the next full listing. So what was already
  // known about the profile side is kept, and only the operation's own side -
  // metadata and config - is taken.
  //
  // Built as a new entry rather than patched into the details: `details` is the
  // reply's own object, which this does not own and which React may hand back
  // here more than once - an updater has to be able to run twice over the same
  // state and leave the same answer.
  const applyInstalledModDetails = useCallback(
    (
      modId: string,
      details: InstalledModDetails,
      profileFieldsKnown = true
    ) => {
      applyWrite(modId, (known) =>
        profileFieldsKnown || !known
          ? details
          : {
              ...details,
              latestVersion: known.latestVersion,
              userRating: known.userRating,
            }
      );
    },
    [applyWrite]
  );

  // A whole config, for the mod whose config the echo below could not be applied
  // to.
  const { getModConfig } = useGetModConfig();
  const readModConfig = useCallback(
    async (modId: string) => {
      const result = await getModConfig({ modId });
      if (result.status !== 'reply' || !result.data.config) {
        return;
      }
      const config = result.data.config;
      applyWrite(modId, (entry) => entry && { ...entry, config });
    },
    [getModConfig, applyWrite]
  );

  // A mod's config as changed anywhere else: the Advanced tab's logging
  // switches, an update offer turned down or let back in.
  //
  // The echo is the patch that was written, which is a config only over the one
  // it was written against. A mod on disk that was never compiled has no config
  // here to patch - the host writes it one on the first such write - so there
  // the patch is dropped and the whole thing read back instead. Merging into
  // nothing would leave a partial config standing as a whole one; dropping it
  // outright would leave the write the user just made off the screen until the
  // next listing.
  useSetNewModConfig(
    useCallback(
      (data) => {
        const { modId, config: newConfig } = data;
        const entry = heldMod(modId);
        if (!entry) {
          return;
        }
        if (!entry.config) {
          void readModConfig(modId);
          return;
        }
        applyWrite(modId, (patched) =>
          patched?.config
            ? { ...patched, config: { ...patched.config, ...newConfig } }
            : patched
        );
      },
      [heldMod, readModConfig, applyWrite]
    )
  );

  // What the host has found out about the installed mods since it last said: a
  // fresh update check, a rating that has come back, or a mod written to
  // somewhere this window is not - the CLI, or a second window. The messages
  // above carry only this window's own doing, so this is where another process's
  // reaches the screen; without it a badge would stand until the next full
  // listing, over a mod on disk that has moved on.
  //
  // Written through the plain setter rather than as this window's write:
  // marking every mod the host names would leave the next listing unable to say
  // that a mod is gone.
  //
  // It carries every term an offer is reached from, and each is taken: an answer
  // over a mix of what the host has just read and what this window last heard is
  // one neither of them would give. So the version the mod is AT is taken like
  // the rest - another process is as free to install over a mod as to write its
  // config, and this is the message that says so.
  useUpdateInstalledModsDetails(
    useCallback(
      (data) => {
        const installedModsDetails = data.details;
        setInstalledMods((prev) =>
          prev &&
          produce(prev, (draft) => {
            for (const [modId, updatedDetails] of Object.entries(
              installedModsDetails
            )) {
              const entry = draft[modId];
              if (entry) {
                entry.latestVersion = updatedDetails.latestVersion;
                entry.userRating = updatedDetails.userRating;
                // A mod on disk whose source the host cannot read has no
                // metadata here, and one that was never compiled no config - so
                // for either there is nothing to write, and nothing to invent to
                // write it into. The host names no version for the first of
                // those anyway, and the empty suppression it names for the
                // second is what the absent one already reads as.
                if (entry.metadata) {
                  entry.metadata.version =
                    updatedDetails.installedVersion ?? undefined;
                }
                if (entry.config) {
                  entry.config.updatesDisabledForVersion =
                    updatedDetails.updatesDisabledForVersion;
                  // Not a term of the offer, but the catalog sync or the update
                  // check in another process is what nukes or restores a mod,
                  // and this is the only news of it.
                  entry.config.nuked = updatedDetails.nuked;
                }
              }
            }
          })
        );
      },
      [setInstalledMods]
    )
  );

  return {
    installedMods,
    applyInstalledModDetails,
    applyInstalledModsListing,
    modWriteMark,
    modWrittenSince,
    applyWrite,
    readModConfig,
  };
}

type Args = {
  // Told after a mod is removed, for an owner with somewhere to be when the mod
  // it was showing goes away.
  onModDeleted?: (modId: string) => void;
  readListingOnMount?: boolean;
};

/**
 * The state above, and every action a browser runs on a mod. Both browsers hold
 * the same thing - the home screen the machine, the repository browser the
 * machine's side of a listing - so following the replies is one thing here
 * rather than a copy per screen, which is what leaves one able to fall behind
 * the other.
 *
 * Each action takes its own reply, so a screen can run several at once - a
 * selection turned off in one go is a request per mod - and each mod is left as
 * the host answered for that mod.
 */
export function useInstalledMods({
  onModDeleted,
  readListingOnMount,
}: Args = {}) {
  const {
    installedMods,
    applyInstalledModDetails,
    applyInstalledModsListing,
    modWriteMark,
    modWrittenSince,
    applyWrite,
    readModConfig,
  } = useInstalledModsState({ readListingOnMount });

  const { installMod: postInstallMod, installModPending } = useInstallMod();

  // An error beside a non-null details is the host saying the mod is on the
  // machine but it could not read the mod back after putting it there - so the
  // details are the operation's own report and not the whole entry.
  const installMod = useCallback(
    async (data: InstallModData) => {
      const result = await postInstallMod(data);
      if (result.status === 'reply' && result.data.installedModDetails) {
        applyInstalledModDetails(
          data.modId,
          result.data.installedModDetails,
          !result.data.error
        );
      }
      return result;
    },
    [postInstallMod, applyInstalledModDetails]
  );

  const { compileMod: postCompileMod, compileModPending } = useCompileMod();

  const compileMod = useCallback(
    async (data: CompileModData) => {
      const result = await postCompileMod(data);
      if (result.status === 'reply' && result.data.compiledModDetails) {
        applyInstalledModDetails(
          data.modId,
          result.data.compiledModDetails,
          !result.data.error
        );
      }
      return result;
    },
    [postCompileMod, applyInstalledModDetails]
  );

  const { enableMod: postEnableMod, enableModPending } = useEnableMod();

  // A refusal changes nothing on the machine, so nothing here. A mod refused for
  // being nuked is one this window did not know was, the flag having come after
  // the mod was last read, so its config is read again to say so.
  const enableMod = useCallback(
    async (data: EnableModData) => {
      const result = await postEnableMod(data);
      if (result.status === 'reply' && result.data.error?.code === 'MOD_NUKED') {
        void readModConfig(data.modId);
      }
      if (result.status !== 'reply' || !result.data.succeeded) {
        return result;
      }
      const enabled = result.data.enabled;
      applyWrite(data.modId, (entry) =>
        entry?.config
          ? { ...entry, config: { ...entry.config, disabled: !enabled } }
          : entry
      );
      return result;
    },
    [postEnableMod, applyWrite, readModConfig]
  );

  const { deleteMod: postDeleteMod, deleteModPending } = useDeleteMod();

  const deleteMod = useCallback(
    async (data: DeleteModData) => {
      const result = await postDeleteMod(data);
      if (result.status !== 'reply' || !result.data.succeeded) {
        return result;
      }
      onModDeleted?.(data.modId);
      applyWrite(data.modId, () => undefined);
      return result;
    },
    [postDeleteMod, onModDeleted, applyWrite]
  );

  const { updateModRating: postUpdateModRating } = useUpdateModRating();

  const updateModRating = useCallback(
    async (data: UpdateModRatingData) => {
      const result = await postUpdateModRating(data);
      if (result.status !== 'reply' || !result.data.succeeded) {
        return result;
      }
      const rating = result.data.rating;
      applyWrite(data.modId, (entry) => entry && { ...entry, userRating: rating });
      return result;
    },
    [postUpdateModRating, applyWrite]
  );

  return {
    installedMods,
    applyInstalledModDetails,
    applyInstalledModsListing,
    modWriteMark,
    modWrittenSince,
    installMod,
    installModPending,
    compileMod,
    compileModPending,
    enableMod,
    enableModPending,
    deleteMod,
    deleteModPending,
    updateModRating,
  };
}
