import { isLocalModId } from '@app/utils';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  useGetModSourceData,
  useGetRepositoryModSourceData,
  useUpdateModConfig,
} from '@app/webviewIPC';
import {
  formatSuppression,
  type ModMetadata,
  type RepositoryDetails,
  type UpdateSuppression,
} from '@app/webviewIPCMessages';
import { type InstalledModEntry } from '../shared/installedMod';
import { resolveUpdateOffer, type UpdateOffer } from '../shared/updateOffer';
import {
  ModDetailsView,
  type ModSourceData,
  type RepositoryStatus,
} from './ModDetails.View';
import {
  resolveHeaderActions,
  type ModDetailsState,
  type ShownVersion,
} from './modDetailsState';

export type RepositoryModDetails = {
  metadata?: ModMetadata;
  details?: RepositoryDetails;
};

// What an owner can let its screen do with the mod. The three that run on a
// source are each absent where the owner has no such action: a screen showing
// only mods that are already installed has nothing to install, and an absent
// callback takes the action off the header rather than showing one that cannot
// run.
export type ModActionCallbacks = {
  installMod?: (modSource: string) => void;
  updateMod?: (modSource: string) => void;
  forkModFromSource?: (modSource: string) => void;
  compileMod: () => void;
  enableMod: (enable: boolean) => void;
  editMod: () => void;
  forkMod: () => void;
  deleteMod: () => void;
  updateModRating: (newRating: number) => void;
};

// Extension-only state and callbacks
export type ExtensionProps = {
  // The mod as the host lists it, absent for one that is not on the machine. The
  // entry whole rather than the parts this screen reads: `latestVersion` is the
  // version the repository holds as the host last cached it, which is what lets
  // the offer here be the same answer the list behind this screen shows - and
  // shows it from the first frame, rather than after a fetch of its own.
  installedModDetails?: InstalledModEntry;
  // Whether to read the repository's side of the mod: its source, and with it the
  // latest version to show and to act on. The one thing that says so - an owner
  // handing in `repositoryModDetails` asks for this too, rather than the listing
  // standing in as a second way to ask, which left two answers to one question
  // and no saying which of them a reader was looking at.
  loadRepositoryData?: boolean;
  // Whether the tab on screen is kept for the next time the screen is built,
  // rather than starting at the mod's details. For an owner whose screen is
  // drawn from scratch again on its own - the editor's preview, which the host
  // reloads in place - where starting over is not a move the reader made.
  remembersActiveTab?: boolean;
  // Absent for an owner that wires none of the mod's actions - the editor's
  // preview - which gets the mod's details without the buttons, the version
  // list, the rating and the way back rather than a row of actions that would
  // each report itself unavailable. It says nothing about the tabs, which reach
  // the host on their own.
  actions?: ModActionCallbacks;
};

interface Props {
  modId: string;
  repositoryModDetails?: RepositoryModDetails;
  goBack?: () => void;

  // Extension-specific props (all grouped together)
  extensionProps?: ExtensionProps;
}

export function ModDetailsExtension({ modId, repositoryModDetails, goBack, extensionProps }: Props) {
  if (!extensionProps) {
    throw new Error('ModDetailsExtension requires extensionProps');
  }

  // Extract extension data
  const installedModDetails = extensionProps.installedModDetails;
  const loadRepositoryData = extensionProps.loadRepositoryData;
  const modActions = extensionProps.actions;

  // One source per version this screen can show, keyed by the view that shows
  // it, so a view added to the state is one this has to answer for.
  //
  // The picked slot carries the version it holds, unlike the other two, whose
  // view names one version for as long as it stands. What is picked moves while
  // reads are in flight, so an answer that lands is about a version rather than
  // about the slot: saying which one leaves the reader to tell an answer it asked
  // for from one it has moved off.
  const [sourceDataMap, setSourceDataMap] = useState<{
    installed: ModSourceData | null;
    latest: ModSourceData | null;
    picked: { version: string; data: ModSourceData } | null;
  }>({
    installed: null,
    latest: null,
    picked: null,
  });

  // The view the user asked for, as the one view it is rather than as a pick
  // and a selection that have to be read in the right order. Null until they
  // ask for one, which leaves the mod described from wherever it sits.
  const [chosenView, setChosenView] = useState<ShownVersion | null>(null);
  const pickedView = chosenView?.kind === 'picked' ? chosenView : null;
  const pickedVersion = pickedView?.version ?? null;

  // When each version was published, as the version list last delivered them.
  // They describe the mod's versions rather than the choice made among them, so
  // they are held apart from that choice: which way a move goes is a question
  // about two versions, and it can be asked of the version the offer names as
  // readily as of one picked by hand. Empty until the list has been opened,
  // which leaves a move unable to say it goes backwards - and calling it an
  // update, which is what it is called when nothing is known.
  const [versionTimestamps, setVersionTimestamps] = useState<
    Record<string, number>
  >({});

  // What a read that lands is judged against, rather than what its request closed
  // over: the mod the screen is on.
  const modIdRef = useRef(modId);
  useEffect(() => {
    modIdRef.current = modId;
  });

  // IPC Hook: Fetch installed mod source
  const { getModSourceData } = useGetModSourceData();
  const readInstalledSource = useCallback(async () => {
    const result = await getModSourceData({ modId });
    if (result.status !== 'reply' || modIdRef.current !== modId) {
      return;
    }
    setSourceDataMap(prev => ({ ...prev, installed: result.data.data }));
  }, [getModSourceData, modId]);

  // Read for any mod that is on the machine, not only one whose metadata parsed:
  // a source the host cannot read is answered with a null source, and that answer
  // is what tells the screen the read failed. Asking only where the metadata
  // arrived leaves that reply unsent, and "never asked for" and "still on its
  // way" come out as the same empty value.
  const hasInstalledMod = !!installedModDetails;
  // What the read keys on. Not the details object, whose identity every config
  // write changes; the metadata's, which moves on the two things that leave a new
  // source on disk to read - an install or a recompile reporting what it landed,
  // and a fresh listing, which is where a local mod edited outside this window
  // arrives. A config write leaves it alone, immer sharing it through.
  const installedModMetadata = installedModDetails?.metadata;
  useEffect(() => {
    if (hasInstalledMod) {
      void readInstalledSource();
    }
  }, [hasInstalledMod, installedModMetadata, readInstalledSource]);

  // IPC Hook: Fetch the repository's source, at the latest version or a named one
  const { getRepositoryModSourceData } = useGetRepositoryModSourceData();
  // A read for no version is of the latest one, which stands whatever is picked.
  // A versioned one goes to the picked slot under the version it was asked for,
  // which is what leaves one the pick has since moved off readable as the stale
  // thing it is.
  const readRepositorySource = useCallback(
    async (version?: string) => {
      const result = await getRepositoryModSourceData(
        version ? { modId, version } : { modId }
      );
      if (result.status !== 'reply' || modIdRef.current !== modId) {
        return;
      }
      const data = result.data.data;
      setSourceDataMap(prev =>
        version
          ? { ...prev, picked: { version, data } }
          : { ...prev, latest: data }
      );
    },
    [getRepositoryModSourceData, modId]
  );

  // Everything above describes one mod, and the mod on screen can change without
  // this screen being built again. All of it is dropped together: a read still
  // on its way would otherwise leave the previous mod's source, and the version
  // picked out of its list, under the new mod's header.
  const [shownModId, setShownModId] = useState(modId);
  if (shownModId !== modId) {
    setShownModId(modId);
    setSourceDataMap({ installed: null, latest: null, picked: null });
    setChosenView(null);
    setVersionTimestamps({});
  }

  // Whether this screen looks at the repository at all, which is the owner's to
  // say and which it says once: a listing handed in is data about a mod, not an
  // answer about whether to go and read one, and an owner that hands one in asks
  // for the read as well. A flag rather than the listing object for the fetch's
  // own sake too - a caller that builds that object inline hands over a new one
  // on every render, and this fetch would go out again with each of them.
  const wantsRepositorySource = !!loadRepositoryData;
  useEffect(() => {
    if (wantsRepositorySource) {
      void readRepositorySource();
    }
  }, [readRepositorySource, wantsRepositorySource]);

  // The mod as it sits on the machine, and which of its versions is on screen.
  // Built as the one pair rather than as two values that have to agree: which
  // views there are to show follows from whether there is an installed side, so
  // the arm that has none never names it. The latest view applies only while
  // there is a repository side to show; otherwise it is ignored so a stale
  // choice never drives the view.
  const state: ModDetailsState = ((): ModDetailsState => {
    if (!installedModDetails) {
      return { installed: null, shown: pickedView ?? { kind: 'latest' } };
    }
    return {
      installed: installedModDetails,
      shown:
        pickedView ??
        (wantsRepositorySource && chosenView?.kind === 'latest'
          ? { kind: 'latest' }
          : { kind: 'installed' }),
    };
  })();
  const { shown } = state;

  // The source read for the version picked by name, which is the picked slot only
  // while it holds the version that is picked: anything else in there answers
  // about a version this screen has moved off, and reads as the absent source it
  // is until its own answer lands.
  const pickedSourceData =
    shown.kind === 'picked' && sourceDataMap.picked?.version === shown.version
      ? sourceDataMap.picked.data
      : null;

  // The source behind the version on screen.
  const modSourceData =
    shown.kind === 'picked' ? pickedSourceData : sourceDataMap[shown.kind];

  // What the mod was listed as. A picked version is known only from its own read.
  const listedMetadata =
    shown.kind === 'installed'
      ? installedModDetails?.metadata
      : shown.kind === 'latest'
        ? repositoryModDetails?.metadata
        : undefined;

  // The read's own metadata, falling back to the listing's - per value rather
  // than per record, a read that failed answering with every field absent.
  const modMetadata: ModMetadata =
    modSourceData?.metadata ?? listedMetadata ?? {};

  // The source an action would put on the machine: the version asked for by
  // name, or the one the repository is offering. Not the source on screen - a
  // screen reading the installed version acts on what would replace it. Absent
  // while it is on its way, which is what leaves the action disabled.
  const selectedModSourceData =
    shown.kind === 'picked' ? pickedSourceData : sourceDataMap.latest;
  const actionSource = selectedModSourceData?.source ?? null;

  // Version selector handlers. Neither clears the picked slot: what is in it says
  // which version it is for, so a slot left behind is read as the stale answer it
  // is - and where it turns out to be the version picked next, it is that
  // version's source and stands.
  const handleShowVersion = useCallback((kind: 'installed' | 'latest') => {
    setChosenView({ kind });
  }, []);

  const handleVersionSelect = useCallback((version: string, timestamps: Record<string, number>) => {
    setChosenView({ kind: 'picked', version });
    setVersionTimestamps(timestamps);
    void readRepositorySource(version);
  }, [readRepositorySource]);

  const { updateModConfig } = useUpdateModConfig();

  // The config this component is handed is refreshed by the host's setNewModConfig
  // echo, not by this write's reply - so there is nothing here to take off it.
  const storeSuppression = useCallback(
    (stored: string) => {
      void updateModConfig({
        modId,
        config: { updatesDisabledForVersion: stored },
      });
    },
    [modId, updateModConfig]
  );

  // The version the listing this screen was handed names, read off it rather
  // than held as the listing itself: a caller that builds that object inline
  // hands over a new one on every render, and the memo below would never hold.
  const listedRepositoryVersion = repositoryModDetails?.metadata?.version;

  // The version the host last cached for the mod on the machine, which is its own
  // answer about the repository. Read off the entry for the same reason as above:
  // every config write hands over a new entry, and this moves only when a check
  // comes back.
  const cachedRepositoryVersion = installedModDetails?.latestVersion;

  // Where the repository side of the mod stands. The two facts it carries are
  // apart because they answer apart: the version is named by whichever side
  // named it first, while whether the source can be read is what the read itself
  // says. Conflating them had a failed read report no version at all, and had a
  // read that failed under a listing report itself as one that succeeded.
  const repositoryStatus = useMemo((): RepositoryStatus | null => {
    if (!wantsRepositorySource) {
      return null;
    }
    return {
      read: !sourceDataMap.latest
        ? 'loading'
        : sourceDataMap.latest.source
          ? 'loaded'
          : 'failed',
      // For a mod on the machine the version is the host's `latestVersion` -
      // what every list reaches its badge and its count from - until the source
      // is read. The listing this screen was handed gets no say, and not because
      // it is the staler of the two: it can be the fresher, the host fetching a
      // catalog live around a repository version it had cached. Freshness is not
      // what orders these.
      //
      // The read wins as the artifact rather than as an opinion about it: it is
      // what the move installs, so an offer naming any other version is the
      // screen saying one thing and doing another. The listing installs nothing,
      // and reaches only one of the two screens that open this one - the home
      // screen hands one over for a mod that is NOT on the machine and never for
      // one that is - so ordering on it had one mod answer differently depending
      // on which list it was opened from.
      //
      // A mod that is not on the machine has no answer of the host's to prefer,
      // and there the listing is what names the version.
      version:
        sourceDataMap.latest?.metadata?.version ??
        (hasInstalledMod ? (cachedRepositoryVersion ?? undefined) : listedRepositoryVersion),
    };
  }, [
    listedRepositoryVersion,
    wantsRepositorySource,
    sourceDataMap.latest,
    hasInstalledMod,
    cachedRepositoryVersion,
  ]);

  // What each side of the mod is at.
  const installedVersion = installedModDetails?.metadata?.version;
  const repositoryVersion = repositoryStatus?.version;

  // Two views naming the same version are one view under two names, and the one
  // further in - the machine over the repository, either over a picked name - is
  // where that version is described from. This is what an install lands as, read
  // off the versions rather than latched onto the request going out: neither
  // side reports a version it does not hold, so one that fails moves nothing.
  if (shown.kind === 'picked') {
    if (installedVersion && shown.version === installedVersion) {
      handleShowVersion('installed');
    } else if (repositoryVersion && shown.version === repositoryVersion) {
      handleShowVersion('latest');
    }
  } else if (
    shown.kind === 'latest' &&
    installedVersion &&
    repositoryVersion === installedVersion
  ) {
    handleShowVersion('installed');
  }

  // The suppression the user stored, read from the INSTALLED config rather than
  // the viewed one: it belongs to the mod, and the other views are the same mod
  // at another version.
  const storedSuppression =
    installedModDetails?.config?.updatesDisabledForVersion ?? '';

  // What the mod's own state and the repository's between them amount to: the
  // same three terms, through the same rule, that the badge and the count behind
  // this screen read, so a mod reads the same way here as in the list it was
  // opened from. A screen that never asked for a repository side names no
  // version there, which is what leaves an offer out of the answer.
  //
  // For a mod that is on the machine: an offer is of a version to replace the
  // one installed, and a suppression is stored on a config there is none of.
  const offer: UpdateOffer = installedModDetails
    ? resolveUpdateOffer({
        installedVersion,
        repositoryVersion,
        storedSuppression,
      })
    : { kind: 'none' };

  // Which way the move an update action would make goes: to the version asked
  // for by name, or to the one the repository is offering.
  //
  // Only a version picked out of the list can be said to go backwards, that being
  // where the timestamps come from. The offer's own version is not worth a fetch
  // to settle: the repository lists its latest last, so an offer that goes
  // backwards is the repository being wrong, and "Update" is the right word for
  // every answer it gives that is not.
  const isDowngrade = ((): boolean => {
    const movingTo =
      shown.kind === 'picked' ? shown.version : repositoryVersion;
    if (!installedVersion || !movingTo) {
      return false;
    }
    const installedAt = versionTimestamps[installedVersion];
    const movingToAt = versionTimestamps[movingTo];
    return (
      installedAt !== undefined &&
      movingToAt !== undefined &&
      movingToAt < installedAt
    );
  })();

  // The actions the header leads with, resolved only for a screen that can run
  // them: one that wires none has no action to resolve, and the pair is what
  // says so in one place.
  const headerActions = modActions
    ? {
        actions: resolveHeaderActions({
          state,
          offer,
          isDowngrade,
          isLocalMod: isLocalModId(modId),
          can: {
            install: !!modActions.installMod,
            update: !!modActions.updateMod,
            forkFromSource: !!modActions.forkModFromSource,
          },
          hasSource: !!actionSource,
        }),
        callbacks: {
          installMod: runOnActionSource(modActions.installMod),
          updateMod: runOnActionSource(modActions.updateMod),
          forkModFromSource: runOnActionSource(modActions.forkModFromSource),
          disableUpdates: (suppression: UpdateSuppression) =>
            storeSuppression(formatSuppression(suppression)),
          allowUpdates: () => storeSuppression(''),
          compileMod: modActions.compileMod,
          enableMod: modActions.enableMod,
          editMod: modActions.editMod,
          forkMod: modActions.forkMod,
          deleteMod: modActions.deleteMod,
          updateModRating: modActions.updateModRating,
        },
      }
    : null;

  // Runs one of the owner's actions on the source an action would put on the
  // machine. Whether each is offered at all, and whether it is offered without
  // running, is resolved from that same source above - which is what leaves
  // this with nothing to run on and nothing to do.
  function runOnActionSource(run: ((modSource: string) => void) | undefined) {
    return () => {
      if (run && actionSource) {
        run(actionSource);
      }
    };
  }

  const extensionViewProps = {
    // The mod as it sits on the machine and which version is on screen, which
    // the tabs and the version selector read as well as the header.
    state,
    // What the header draws and what runs behind it, or null for a screen that
    // wires none of it - which is also what takes the version selector away.
    headerActions,
    remembersActiveTab: !!extensionProps.remembersActiveTab,

    // Version selector state
    repositoryStatus,
    onShowVersion: handleShowVersion,
    onVersionSelect: handleVersionSelect,
  };

  return (
    <ModDetailsView
      modId={modId}
      goBack={goBack}
      modMetadata={modMetadata}
      repositoryDetails={
        (shown.kind === 'latest' && repositoryModDetails?.details) || undefined
      }
      modSourceData={modSourceData}
      installedModSourceData={sourceDataMap.installed}
      selectedModSourceData={selectedModSourceData}
      extensionViewProps={extensionViewProps}
      onRetryLoad={() => {
        // The failure this sits under can be either side's - the tab that diffs
        // them shows one Result for both - and the screen does not carry which,
        // so each side the view could be waiting on is asked for again. The one
        // that succeeded answers with what it already had.
        if (hasInstalledMod) {
          void readInstalledSource();
        }
        if (pickedVersion) {
          void readRepositorySource(pickedVersion);
        } else if (wantsRepositorySource) {
          void readRepositorySource();
        }
      }}
    />
  );
}
