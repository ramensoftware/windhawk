import { AppUISettingsContext } from '@app/appUISettings';
import { DropdownModal, InputWithContextMenu } from '@app/components/InputWithContextMenu';
import { useNavigationBlock } from '@app/navigationBlock';
import {
  getDisplayModId,
  isLocalModId,
  isMobile,
  readStoredValue,
  shuffleArray,
  writeStoredValue,
} from '@app/utils';
import {
  editMod,
  forkMod,
  useGetFeaturedMods,
  useGetInstalledMods,
  useReloadInstalledMods,
} from '@app/webviewIPC';
import {
  type ModMetadata,
  type RepositoryDetails,
} from '@app/webviewIPCMessages';
import { faFilter, faGripVertical, faHdd, faList, faSearch, faStar } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, Empty, type InputRef, Modal, Spin } from 'antd';
import { Fragment, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';
import styled, { css } from 'styled-components';
import { ModDetails } from '../mod-details';
import { ModCard } from '../shared';
import ContentHeldStill from '../shared/ContentHeldStill';
import { type InstalledModEntry } from '../shared/installedMod';
import { compareToRepository, modHasUpdateOnOffer } from '../shared/updateOffer';
import useKeyboardShortcut, { isTypingTarget } from '../shared/useKeyboardShortcut';
import {
  ModGroupHeader,
  ModGroupMoveModal,
  ModGroupRenameModal,
  type ModGroup,
  type ModGroupBlock,
  type ModGroupDestination,
  partitionByGroup,
  useModGroups,
} from './groups';
import { useModOperation } from './modOperation';
import ModOperationModal from './ModOperationModal';
import ModsTableBlock, {
  modTableSorters,
  type ModsTableActions,
  type ModTableRowData,
  type ModTableSort,
} from './ModsTableBlock';
import { actionTargets, ModSelectionBar, useModSelection } from './selection';
import { ModUpdateWizard, UpdatesAvailableBar } from './update-wizard';
import { useCancelModOperation } from './useCancelModOperation';
import { useInstalledMods } from './useInstalledMods';

// Where the grid/list choice is kept for the next visit. Anything else stored
// reads as the grid, which is what an unwritten key gives.
const VIEW_MODE_STORAGE_KEY = 'modsBrowserViewMode';

const SectionHeader = styled.div`
  display: flex;
  justify-content: space-between;
  align-items: start;
  margin-top: 20px;
`;

const SectionIcon = styled(FontAwesomeIcon)`
  margin-inline-end: 3px;
`;

const SearchFilterContainer = styled.div`
  display: flex;
  gap: 10px;
  margin-top: 12px;
  margin-bottom: 20px;
`;

const SearchFilterInput = styled(InputWithContextMenu)`
  > .ant-input-prefix {
    margin-inline-end: 8px;
  }
`;

const IconButton = styled(Button)`
  padding-inline-start: 0;
  padding-inline-end: 0;
  min-width: 40px;
`;

const ModsContainer = styled.div<{ $extraBottomPadding?: boolean }>`
  flex: 1;
  ${({ $extraBottomPadding }) => css`
    padding-bottom: ${$extraBottomPadding ? 70 : 20}px;
  `}
`;

const ModsGrid = styled.div`
  display: grid;
  grid-template-columns: repeat(
    auto-fill,
    calc(min(400px - 20px * 4 / 3, 100%))
  );
  gap: 20px;
  justify-content: center;
`;

// Every list the installed section is drawn as, in the order they are on
// screen: the mods in no group, then one block per group.
//
// Layout-neutral at its leading edge - no margin, no padding, no gap of its own
// before the first block. That is not a style choice: mod-updates.cy.ts measures
// the distance from the search row's bottom to the top of the element carrying
// installed-mods and asserts it equals the gap above the search row. The space
// between blocks belongs to the blocks.
const BlocksContainer = styled.div``;

// A group's header and the mods under it. The gap above it is the grid's own,
// so a group reads as the next list down rather than as part of the one before
// it; the first block on screen has nothing to be spaced from.
const GroupBlock = styled.div`
  margin-top: 20px;

  &:first-child {
    margin-top: 0;
  }
`;

// What stands where a group's mods would be. A group with nothing in it is
// still a group: its name and its menu are the only way to rename or delete it.
const EmptyGroupLine = styled.div`
  color: var(--whui-text-secondary);
`;

const ExploreModsButton = styled(Button)`
  height: 100%;
  font-size: 22px;
`;

const ProgressSpin = styled(Spin)`
  display: block;
  margin-inline-start: auto;
  margin-inline-end: auto;
  font-size: 32px;
`;

type FeaturedModDetailsType = {
  metadata: ModMetadata;
  details: RepositoryDetails;
};

// Which sort a block's table is under: the ungrouped block is '', every other
// key is a group id.
function blockSortKey(block: ModGroupBlock) {
  return block.group?.id ?? '';
}

// One block's mods in the order its own table lists them, which is what the
// filter left until a column header sorts them into an order of its own. Ids
// rather than rows: the rows are built once for the screen and looked up here.
function sortBlockModIds(
  modIds: string[],
  sort: ModTableSort | undefined,
  rowsById: Map<string, ModTableRowData>
) {
  if (!sort) {
    return modIds;
  }

  const compare = modTableSorters[sort.key];
  const direction = sort.order === 'descend' ? -1 : 1;
  return [...modIds].sort((modIdA, modIdB) => {
    const rowA = rowsById.get(modIdA);
    const rowB = rowsById.get(modIdB);
    return rowA && rowB ? compare(rowA, rowB) * direction : 0;
  });
}

function modMatchesFilter(
  modId: string,
  mod: InstalledModEntry,
  filterWords: string[],
  filterOptions: Set<string>
): boolean {
  const textMatch = filterWords.every(
    (filterWord) =>
      modId.toLowerCase().includes(filterWord) ||
      mod.metadata?.name?.toLowerCase().includes(filterWord) ||
      mod.metadata?.description?.toLowerCase().includes(filterWord)
  );
  if (!textMatch) {
    return false;
  }

  if (filterOptions.has('enabled') && (!mod.config || mod.config.disabled)) {
    return false;
  }

  if (filterOptions.has('disabled') && mod.config && !mod.config.disabled) {
    return false;
  }

  if (
    filterOptions.has('update-available') &&
    !modHasUpdateOnOffer(modId, mod)
  ) {
    return false;
  }

  return true;
}

interface Props {
  ContentWrapper: React.ComponentType<
    React.ComponentPropsWithoutRef<'div'> & { $hidden?: boolean }
  >;
}

function ModsBrowserLocal({ ContentWrapper }: Props) {
  const { t } = useTranslation();

  const navigate = useNavigate();

  const { modType: displayedModType, modId: displayedModId } = useParams<{
    modType: string;
    modId: string;
  }>();

  // The mods on the machine, and every message the host moves one with.
  const {
    installedMods,
    applyInstalledModsListing,
    modWriteMark,
    applyInstalledModDetails,
    installMod,
    installModPending,
    compileMod,
    compileModPending,
    enableMod,
    enableModPending,
    deleteMod,
    deleteModPending,
    updateModRating,
  } = useInstalledMods({
    // The details pane of a mod that has just been removed is a pane about
    // nothing.
    onModDeleted: useCallback(
      (modId: string) => {
        if (displayedModType === 'local' && displayedModId === modId) {
          navigate('/', { replace: true });
        }
      },
      [displayedModId, displayedModType, navigate]
    ),
  });

  const [featuredMods, setFeaturedMods] = useState<
    Record<string, FeaturedModDetailsType> | undefined | null
  >(undefined);

  const [filterText, setFilterText] = useState('');
  const [filterOptions, setFilterOptions] = useState<Set<string>>(new Set());
  const [filterDropdownOpen, setFilterDropdownOpen] = useState(false);
  const [confirmModalOpen, setConfirmModalOpen] = useState(false);
  const [updateWizardOpen, setUpdateWizardOpen] = useState(false);
  const [viewMode, setViewMode] = useState<'grid' | 'list'>(() =>
    readStoredValue(VIEW_MODE_STORAGE_KEY) === 'list' ? 'list' : 'grid'
  );
  const [searchInputFocused, setSearchInputFocused] = useState(!isMobile);

  // Which column each block's table is sorted by, held here rather than inside
  // antd or inside the tables: the mods are ordered from it, and so is what the
  // selection ranges over - which is a thing only the screen holding every block
  // can put together. An entry left behind by a group that has since been
  // deleted is never read again, no group being handed an id another one had;
  // pruning it would be a second place that has to know when a group goes away.
  const [tableSorts, setTableSorts] = useState<Record<string, ModTableSort>>(
    {}
  );

  const {
    groups,
    assign: assignToModGroup,
    rename: renameModGroup,
    remove: removeModGroup,
    swap: swapModGroups,
    setCollapsed: setModGroupCollapsed,
  } = useModGroups();

  // Where a selection is being sent, and which group is being renamed. Neither
  // joins modalIsOpen: that set holds the route back for things that would be
  // lost or left dangerous by a route change, and these are elements of this
  // screen with no request behind them.
  const [moveModalOpen, setMoveModalOpen] = useState(false);
  const [renameGroupId, setRenameGroupId] = useState<string | null>(null);

  const searchInputRef = useRef<InputRef>(null);

  // Keyboard shortcut: "/" to focus search (desktop only). Not offered while mod
  // details are shown, since the search input is hidden then.
  useKeyboardShortcut(
    !isMobile && !displayedModId,
    (e) => e.key === '/' && !isTypingTarget(e),
    () => searchInputRef.current?.focus()
  );

  const handleViewModeChange = useCallback((mode: 'grid' | 'list') => {
    setViewMode(mode);
    writeStoredValue(VIEW_MODE_STORAGE_KEY, mode);
  }, []);

  // Snapshot-based filtering: the set of mods that pass the filter is captured
  // when the filter criteria (filterOptions/filterText) change and kept stable
  // while mod properties (enabled/disabled) change. This prevents mods from
  // disappearing when the user toggles their state while a filter is active.
  // sourceIds records the mods the snapshot covers, so each mod's decision is
  // frozen once made while mods that appear or disappear (e.g. installed on
  // disk and picked up by a refresh) are still reconciled.
  const [filterSnapshot, setFilterSnapshot] = useState<{
    filterText: string;
    filterOptions: Set<string>;
    sourceIds: Set<string>;
    ids: Set<string>;
  } | null>(null);

  if (installedMods) {
    const currentIds = Object.keys(installedMods);
    const filterChanged =
      !filterSnapshot ||
      filterSnapshot.filterText !== filterText ||
      filterSnapshot.filterOptions !== filterOptions;
    const installedSetChanged =
      !!filterSnapshot &&
      (currentIds.length !== filterSnapshot.sourceIds.size ||
        currentIds.some((modId) => !filterSnapshot.sourceIds.has(modId)));

    if (filterChanged || installedSetChanged) {
      const filterWords = filterText
        .toLowerCase()
        .split(/\s+/)
        .filter((word) => word.length > 0);

      // Reuse decisions from the frozen snapshot; a filter change re-decides
      // every mod. Mods that are gone drop out by iterating currentIds.
      const frozen = filterChanged ? null : filterSnapshot;
      const ids = new Set<string>();
      for (const modId of currentIds) {
        const keep =
          frozen && frozen.sourceIds.has(modId)
            ? frozen.ids.has(modId)
            : modMatchesFilter(
                modId,
                installedMods[modId],
                filterWords,
                filterOptions
              );
        if (keep) {
          ids.add(modId);
        }
      }

      setFilterSnapshot({
        filterText,
        filterOptions,
        sourceIds: new Set(currentIds),
        ids,
      });
    }
  }

  const installedModsFilteredAndSorted = useMemo(() => {
    if (!installedMods) {
      return installedMods;
    }
    if (!filterSnapshot) {
      // Transient render before the snapshot is set; a re-render follows.
      return [];
    }

    return Object.entries(installedMods)
      .filter(([modId]) => filterSnapshot.ids.has(modId))
      .sort((a, b) => {
        const [modIdA, modA] = a;
        const [modIdB, modB] = b;
        const modAIsLocal = isLocalModId(modIdA);
        const modBIsLocal = isLocalModId(modIdB);

        if (modAIsLocal !== modBIsLocal) {
          return modAIsLocal ? -1 : 1;
        }

        const modATitle = (modA.metadata?.name || modIdA).toLowerCase();
        const modBTitle = (modB.metadata?.name || modIdB).toLowerCase();

        if (modATitle < modBTitle) {
          return -1;
        } else if (modATitle > modBTitle) {
          return 1;
        }

        if (modIdA < modIdB) {
          return -1;
        } else if (modIdA > modIdB) {
          return 1;
        }

        return 0;
      });
  }, [installedMods, filterSnapshot]);

  // The listed mods by id, for the blocks below: they hold ids, and a card is
  // drawn from what the mod says about itself.
  const listedModsById = useMemo(
    () => new Map(installedModsFilteredAndSorted ?? []),
    [installedModsFilteredAndSorted]
  );

  // Which mods are in which block, each block in the filtered order. The sort
  // the view applies is applied once, above, and inherited by every block.
  const blocks = useMemo(
    () =>
      partitionByGroup(
        groups,
        (installedModsFilteredAndSorted ?? []).map(([modId]) => modId)
      ),
    [groups, installedModsFilteredAndSorted]
  );

  const filterActive = filterText.trim().length > 0 || filterOptions.size > 0;

  // The groups that have a block on screen, in the order they are drawn in. A
  // heading over nothing during a search says nothing about what matched, so a
  // group whose every mod the filter hid is left out of the list entirely.
  //
  // Which is why reordering is offered against this rather than against the
  // whole group list: swapping with a group nothing on screen shows would move
  // the stored order and leave the screen as it was, so a user pressing Move up
  // would watch the group stay where it is.
  const drawnGroupIds = useMemo(
    () =>
      blocks.flatMap((block) =>
        block.group && !(filterActive && block.modIds.length === 0)
          ? [block.group.id]
          : []
      ),
    [blocks, filterActive]
  );

  // The groups whose fold the user set by hand, and the filter they set it
  // under. A filter draws every group open, so a mod it matched inside a folded
  // group is not left behind a header with nothing under it; a caret pressed
  // over that is still the user's word on the group and takes effect at once.
  // Only the drawing is held here - the fold itself goes to storage either way -
  // so a change of filter puts every group back to what is stored.
  //
  // Reconciled during render, the way the filter snapshot above is, so the
  // blocks are never drawn one frame behind the filter they belong to.
  const [foldsSetByHand, setFoldsSetByHand] = useState<{
    filterText: string;
    filterOptions: Set<string>;
    groupIds: Set<string>;
  }>(() => ({ filterText, filterOptions, groupIds: new Set() }));

  if (
    foldsSetByHand.groupIds.size > 0 &&
    (foldsSetByHand.filterText !== filterText ||
      foldsSetByHand.filterOptions !== filterOptions)
  ) {
    setFoldsSetByHand({ filterText, filterOptions, groupIds: new Set() });
  }

  const groupIsCollapsed = (group: ModGroup) =>
    group.collapsed &&
    (!filterActive || foldsSetByHand.groupIds.has(group.id));

  const toggleGroupCollapsed = (group: ModGroup) => {
    setModGroupCollapsed(group.id, !groupIsCollapsed(group));
    if (filterActive) {
      setFoldsSetByHand((current) => ({
        filterText,
        filterOptions,
        groupIds: new Set(current.groupIds).add(group.id),
      }));
    }
  };

  // The mods an update is waiting for, read off every installed mod rather than
  // off installedModsFilteredAndSorted: the bar and the wizard describe the
  // machine, not the search box, so a user who has typed a filter still gets the
  // true count.
  const updatableMods = useMemo(
    () =>
      Object.entries(installedMods ?? {})
        .filter(([modId, mod]) => modHasUpdateOnOffer(modId, mod))
        .map(([modId, mod]) => ({
          modId,
          name: mod.metadata?.name || getDisplayModId(modId),
          installedVersion: mod.metadata?.version,
        })),
    [installedMods]
  );

  // One row per listed mod, keyed by id: the row objects are built once for the
  // screen rather than once per block, and each block's table takes its own ids
  // out of this in its own order. Built off the filtered list alone, so a sort
  // is a sort of ids and nothing here is rebuilt by one.
  const tableRowsById = useMemo(
    () =>
      new Map<string, ModTableRowData>(
        (installedModsFilteredAndSorted ?? []).map(([modId, mod]) => [
          modId,
          {
            key: modId,
            modId,
            name: mod.metadata?.name || getDisplayModId(modId),
            description: mod.metadata?.description,
            author: mod.metadata?.author,
            version: mod.metadata?.version,
            isLocal: isLocalModId(modId),
            updateAvailable: modHasUpdateOnOffer(modId, mod),
            disabled: mod.config ? mod.config.disabled : true,
            notCompiled: !mod.config,
            mod,
          },
        ])
      ),
    [installedModsFilteredAndSorted]
  );

  // What is on screen, in the order it is on screen. The grid takes the
  // partition as it comes; the list view's tables sort each block into an order
  // the filter never had, and do it per block, so the sort is applied here
  // rather than inside the partition - which has no business knowing about a
  // view.
  const orderedBlocks = useMemo(
    () =>
      viewMode === 'list'
        ? blocks.map((block) => ({
            ...block,
            modIds: sortBlockModIds(
              block.modIds,
              tableSorts[blockSortKey(block)],
              tableRowsById
            ),
          }))
        : blocks,
    [viewMode, blocks, tableSorts, tableRowsById]
  );

  // The listed order, which is what a selection may hold: a mod a filter has
  // hidden cannot be seen and cannot be unchecked, so it is no longer selected.
  // It is also the order a shift-click fills a run in, which is why it is read
  // off the blocks: they are the lists on screen, top to bottom, each in the
  // order its own view puts it in. A run through any other one would take mods
  // the user did not draw a line through. A folded group contributes its mods,
  // being one thing on screen: they stay selected, stay counted, and are taken
  // whole by a range that crosses them.
  // null while there is no list to be trusted - the mods have not arrived, or the
  // filter snapshot has not been set and the list below is transiently empty -
  // since pruning against that would empty the selection on every filter change.
  const visibleModIds = useMemo(
    () =>
      installedModsFilteredAndSorted && filterSnapshot
        ? orderedBlocks.flatMap((block) => block.modIds)
        : null,
    [installedModsFilteredAndSorted, filterSnapshot, orderedBlocks]
  );

  const {
    selectedIds,
    isSelected,
    toggle: toggleSelected,
    setSelection,
    selectAll,
    clear: clearSelection,
  } = useModSelection(visibleModIds);

  // A whole block checked or unchecked from its header, over the selection
  // already made rather than in place of it: the union with the block's listed
  // mods, or the difference. It leaves the shift-anchor where it is - a group is
  // not a place a range starts from, so the next shift-click still fills the run
  // from the mod the user last pointed at.
  const setBlockSelected = useCallback(
    (blockModIds: string[], selected: boolean) => {
      const next = new Set(selectedIds);
      for (const modId of blockModIds) {
        if (selected) {
          next.add(modId);
        } else {
          next.delete(modId);
        }
      }
      setSelection([...next]);
    },
    [selectedIds, setSelection]
  );

  // Read off the listed mods rather than off the selection, so each action's
  // targets come out in the order they are on screen.
  const selectedMods = useMemo(
    () =>
      (installedModsFilteredAndSorted ?? []).filter(([modId]) =>
        selectedIds.has(modId)
      ),
    [installedModsFilteredAndSorted, selectedIds]
  );

  const selectedModIds = useMemo(
    () => selectedMods.map(([modId]) => modId),
    [selectedMods]
  );

  // A dialog is nothing without mods to act on, and the window-focus refetch can
  // take the last of them out from under this one - a mod uninstalled on disk
  // leaves the list, and leaves the selection with it. Dropped during render,
  // the way the filter snapshot reconciles itself, so the dialog is never drawn
  // over a selection it has already lost.
  if (moveModalOpen && selectedModIds.length === 0) {
    setMoveModalOpen(false);
  }

  const selectionTargets = useMemo(
    () =>
      actionTargets(
        selectedMods.map(([modId, mod]) => ({
          modId,
          compiled: !!mod.config,
          disabled: mod.config ? mod.config.disabled : true,
        }))
      ),
    [selectedMods]
  );

  const featuredModsShuffled = useMemo(() => {
    return featuredMods && shuffleArray([...Object.entries(featuredMods)]);
  }, [featuredMods]);

  const featuredModsFilteredAndSorted = useMemo(() => {
    if (!featuredModsShuffled) {
      return featuredModsShuffled;
    }

    const maxFeaturedModsToShow = 5;

    // Return a random sample of non-installed mods.
    const notInstalled = featuredModsShuffled.filter(
      ([modId, mod]) => !installedMods?.[modId]
    );
    return notInstalled.slice(0, maxFeaturedModsToShow);
  }, [featuredModsShuffled, installedMods]);

  const { devModeOptOut } = useContext(AppUISettingsContext);

  // The listing answers for the machine as it stood when it was asked for, which
  // is not the same as when it arrives: the host answers each request on its own
  // and a full enumeration outlasts a single command, so a mod this window has
  // enabled or removed in between is one the listing still describes the old way.
  // The mark the request was sent at is taken here and applied with the reply to
  // it, which is what leaves such a mod alone.
  const { getInstalledMods } = useGetInstalledMods();

  const refreshInstalledMods = useCallback(async () => {
    const at = modWriteMark();
    const result = await getInstalledMods({});
    if (result.status === 'reply') {
      applyInstalledModsListing(result.data.installedMods, at);
    }
  }, [getInstalledMods, modWriteMark, applyInstalledModsListing]);

  const { getFeaturedMods } = useGetFeaturedMods();

  const refreshFeaturedMods = useCallback(async () => {
    const result = await getFeaturedMods({});
    if (result.status === 'reply') {
      setFeaturedMods(result.data.featuredMods);
    }
  }, [getFeaturedMods]);

  useEffect(() => {
    void (async () => {
      await Promise.all([refreshInstalledMods(), refreshFeaturedMods()]);
    })();
  }, [refreshInstalledMods, refreshFeaturedMods]);

  // A local mod is the one copy of itself: with nothing on the machine there is
  // no repository side for its details to fall back to, so the pane would sit
  // over a source that never arrives. The removal this window made is answered by
  // onModDeleted above; this is the mod going away under the refresh below,
  // uninstalled on disk while its details were open. A repository mod needs none
  // of this - it goes on being shown as the repository's, with an install.
  useEffect(() => {
    if (
      installedMods &&
      displayedModId &&
      isLocalModId(displayedModId) &&
      !installedMods[displayedModId]
    ) {
      navigate('/', { replace: true });
    }
  }, [installedMods, displayedModId, navigate]);

  // Local mods can change outside the webview; refresh the installed mods list
  // when the window regains focus. Local-only: does not fetch featured mods or
  // touch the network.
  useEffect(() => {
    const handleWindowFocus = () => {
      refreshInstalledMods();
    };
    window.addEventListener('focus', handleWindowFocus);
    return () => {
      window.removeEventListener('focus', handleWindowFocus);
    };
  }, [refreshInstalledMods]);

  // The same re-read, asked for by the host: in the editor this screen sits in a
  // panel that is not where the typing goes, so a window coming back leaves it
  // without a focus event of its own.
  useReloadInstalledMods(refreshInstalledMods);

  // What the progress modal is covering: named where the operation is posted,
  // since the reply to it goes to the caller that posted it and says nothing to
  // the modal or to the cancel button in it.
  const { operation: modOperation, track: trackModOperation } =
    useModOperation();

  const cancelModOperation = useCancelModOperation({
    installModPending,
    compileModPending,
    operation: modOperation,
  });

  // The batch is several of the commands the single-mod controls already post.
  // Unlike an install, an enable or a delete is a fast host-side operation with
  // no compile behind it, and one hook instance already serves the whole screen
  // with several requests in flight - so the fan-out is the loop and nothing
  // else. Failures report themselves through the notification every caller of
  // these commands already reports through.
  const handleSelectionEnable = (enable: boolean) => {
    for (const modId of enable
      ? selectionTargets.enable
      : selectionTargets.disable) {
      enableMod({ modId, enable });
    }
  };

  // One confirmation naming the count, rather than the card's popconfirm: there
  // is no card to anchor a selection's confirmation to. confirmModalOpen holds
  // the route while it is up - Modal.confirm renders outside the route tree, so
  // a route change would leave it over another page, still able to delete a
  // whole selection of mods when it was finally answered.
  const handleSelectionRemove = () => {
    const modIds = selectionTargets.remove;
    setConfirmModalOpen(true);
    Modal.confirm({
      title: t('modSelection.removeConfirm', { count: modIds.length }),
      okText: t('modSelection.removeConfirmOk'),
      cancelText: t('general.actions.cancel'),
      okButtonProps: { danger: true },
      onOk: () => {
        setConfirmModalOpen(false);
        for (const modId of modIds) {
          deleteMod({ modId });
        }
      },
      onCancel: () => {
        setConfirmModalOpen(false);
      },
      closable: true,
      maskClosable: true,
    });
  };

  // Asked the way a mod's removal is, and held by the same flag for the same
  // reason: Modal.confirm renders outside the route tree, so a route change
  // would leave it over another page. What it names is what deleting a group
  // costs, which is nothing but the group - its mods are back with the
  // ungrouped ones on the next frame.
  const handleDeleteGroup = (groupId: string, modCount: number) => {
    setConfirmModalOpen(true);
    Modal.confirm({
      title:
        modCount === 0
          ? t('modGroups.deleteConfirmEmpty')
          : t('modGroups.deleteConfirm', { count: modCount }),
      okText: t('modGroups.deleteOk'),
      cancelText: t('general.actions.cancel'),
      okButtonProps: { danger: true },
      onOk: () => {
        setConfirmModalOpen(false);
        removeModGroup(groupId);
      },
      onCancel: () => {
        setConfirmModalOpen(false);
      },
      closable: true,
      maskClosable: true,
    });
  };

  const [detailsButtonClicked, setDetailsButtonClicked] = useState(false);

  // What a row's controls do, in one object so the columns are defined once and
  // instantiated per block.
  const tableActions = useMemo<ModsTableActions>(
    () => ({
      devModeOptOut,
      toggleSelected,
      enableMod: (modId, enable) => enableMod({ modId, enable }),
      compileMod: (modId) => trackModOperation({ modId }, compileMod({ modId })),
      deleteMod: (modId) => deleteMod({ modId }),
      openModDetails: (modId) => {
        setDetailsButtonClicked(true);
        navigate('/mods/local/' + modId);
      },
      setConfirmModalOpen,
    }),
    [
      devModeOptOut,
      toggleSelected,
      enableMod,
      compileMod,
      trackModOperation,
      deleteMod,
      navigate,
    ]
  );

  const handleFilterChange = (key: string) => {
    setFilterOptions((prevOptions) => {
      const newOptions = new Set(prevOptions);

      // Handle mutually exclusive filters
      if (key === 'enabled' && newOptions.has('disabled')) {
        newOptions.delete('disabled');
      } else if (key === 'disabled' && newOptions.has('enabled')) {
        newOptions.delete('enabled');
      }

      // Toggle the clicked option
      if (newOptions.has(key)) {
        newOptions.delete(key);
      } else {
        newOptions.add(key);
      }

      return newOptions;
    });
  };

  const handleClearFilters = () => {
    setFilterOptions(new Set());
  };

  // Held while a dialog of this screen's own is up over the list. An operation in
  // flight has its progress and its cancel in one. The removal confirmation is
  // here for a different reason: Modal.confirm renders outside the route tree, so
  // a route change would leave it on screen over another page, still able to
  // delete the mod when it is finally answered. The removal popconfirm on a card
  // is an element of this screen and goes with it, which is why it needs no state
  // of its own. The update wizard holds the route itself, as the import dialog
  // does - blockers compose, and the one that knows what is at stake should be the
  // one that says so.
  const modalIsOpen =
    installModPending || compileModPending || confirmModalOpen;

  useNavigationBlock(modalIsOpen);

  // Escape gives the selection back. Offered only while there is one to give
  // back and while the list is what the key would belong to: a dialog of this
  // screen's own, or the details pane it opens over itself, has a better claim
  // on it. The group dialogs are named separately from modalIsOpen, which is
  // about holding the route rather than about where a keystroke belongs - and
  // the move dialog is the one place where clearing the selection would throw
  // away exactly what the dialog is about to act on.
  const groupModalIsOpen = moveModalOpen || renameGroupId !== null;

  useKeyboardShortcut(
    selectedIds.size > 0 && !modalIsOpen && !groupModalIsOpen && !displayedModId,
    (e) => e.key === 'Escape' && !isTypingTarget(e),
    clearSelection
  );

  if (!installedMods || !installedModsFilteredAndSorted) {
    return null;
  }

  const noInstalledMods = Object.keys(installedMods).length === 0;
  const noFilteredResults = installedModsFilteredAndSorted.length === 0 && !noInstalledMods;

  // Whether the mod on screen is one this screen's own list holds: opened out of
  // it, and still on the machine. Anything else is the repository's mod - one
  // off the featured strip, or one removed out from under its own route.
  const showsInstalledMod =
    displayedModType === 'local' &&
    !!displayedModId &&
    !!installedMods[displayedModId];

  // One card, wherever its block puts it. The blocks hold ids, so the mod is
  // looked up rather than carried along with them.
  const renderModCard = (modId: string) => {
    const mod = listedModsById.get(modId);
    if (!mod) {
      return null;
    }

    return (
      <ModCard
        key={modId}
        modId={modId}
        ribbonText={
          modHasUpdateOnOffer(modId, mod)
            ? (t('mod.updateAvailable') as string)
            : undefined
        }
        title={mod.metadata?.name || getDisplayModId(modId)}
        isLocal={isLocalModId(modId)}
        description={mod.metadata?.description}
        buttons={[
          {
            type: 'action',
            text: t('mod.details'),
            testId: 'mod-card-details',
            onClick: () => {
              setDetailsButtonClicked(true);
              navigate('/mods/local/' + modId);
            },
            badge: (mod.config?.loggingEnabled || mod.config?.debugLoggingEnabled) ? {
              tooltip: t('mod.loggingEnabledInAdvancedTab') as string,
            } : undefined,
          },
          {
            type: 'confirm',
            text: t('mod.remove'),
            testId: 'mod-card-remove',
            confirmText: t('mod.removeConfirm') as string,
            confirmOkText: t('mod.removeConfirmOk') as string,
            confirmCancelText: t('general.actions.cancel') as string,
            confirmIsDanger: true,
            onClick: () => deleteMod({ modId }),
          },
        ]}
        switch={{
          title: mod.config ? undefined : (t('mod.notCompiled') as string),
          checked: mod.config ? !mod.config.disabled : false,
          disabled: !mod.config,
          onChange: (checked) => enableMod({ modId, enable: checked }),
        }}
        selection={{
          checked: isSelected(modId),
          onChange: (checked, shiftKey) =>
            toggleSelected(modId, checked, shiftKey),
          label: t('modSelection.selectMod', {
            name: mod.metadata?.name || getDisplayModId(modId),
          }) as string,
        }}
      />
    );
  };

  // The mods of one block, as whichever view is on screen lists them: its own
  // grid, so two small groups do not share a row, or its own table, so sorting
  // one leaves every other exactly as it was. Either way a block is a list of
  // its own, which is what a group is on this screen.
  const renderBlockMods = (block: ModGroupBlock) => {
    if (viewMode === 'grid') {
      return <ModsGrid>{block.modIds.map(renderModCard)}</ModsGrid>;
    }

    const sortKey = blockSortKey(block);

    return (
      <ModsTableBlock
        // The selection read onto the rows here rather than into the lookup
        // above, so that checking a mod cannot reorder a table, and so that the
        // order the selection ranges over does not wait on the selection itself.
        rows={block.modIds.flatMap((modId) => {
          const row = tableRowsById.get(modId);
          return row ? [{ ...row, selected: isSelected(modId) }] : [];
        })}
        sort={tableSorts[sortKey] ?? null}
        onSortChange={(sort) =>
          setTableSorts((current) => {
            const next = { ...current };
            if (sort) {
              next[sortKey] = sort;
            } else {
              delete next[sortKey];
            }
            return next;
          })
        }
        actions={tableActions}
      />
    );
  };

  const renamedGroup = groups.find((group) => group.id === renameGroupId);

  const handleMoveToGroup = (destination: ModGroupDestination) => {
    assignToModGroup(selectedModIds, destination);
  };

  return (
    <>
      <ContentWrapper $hidden={!!displayedModId}>
        <ModsContainer $extraBottomPadding={!devModeOptOut}>
          <SectionHeader>
            <h2>
              <SectionIcon icon={faHdd} /> {t('home.installedMods.title')}
            </h2>
          </SectionHeader>
          <UpdatesAvailableBar
            count={updatableMods.length}
            onOpen={() => setUpdateWizardOpen(true)}
          />
          {!noInstalledMods && (
            <SearchFilterContainer>
              <SearchFilterInput
                ref={searchInputRef}
                autoFocus={!isMobile}
                data-testid="mods-search"
                prefix={<FontAwesomeIcon icon={faSearch} />}
                placeholder={t(isMobile || searchInputFocused ? 'modSearch.placeholder' : 'modSearch.placeholderWithHint') as string}
                allowClear
                value={filterText}
                onChange={(e) => setFilterText(e.target.value)}
                onFocus={() => setSearchInputFocused(true)}
                onBlur={() => setSearchInputFocused(false)}
              />
              <DropdownModal
                placement="bottomRight"
                trigger={['click']}
                arrow={true}
                open={filterDropdownOpen}
                onOpenChange={setFilterDropdownOpen}
                menu={{
                  items: [
                    {
                      label: t('home.filter.enabled'),
                      key: 'enabled',
                    },
                    {
                      label: t('home.filter.disabled'),
                      key: 'disabled',
                    },
                    {
                      label: t('home.filter.updateAvailable'),
                      key: 'update-available',
                    },
                    {
                      type: 'divider',
                    },
                    {
                      label: t('home.filter.clearFilters'),
                      key: 'clear-filters',
                    },
                  ],
                  selectedKeys: Array.from(filterOptions),
                  onClick: (e) => {
                    if (e.key === 'clear-filters') {
                      handleClearFilters();
                      setFilterDropdownOpen(false);
                    } else {
                      handleFilterChange(e.key);
                      // Keep dropdown open for filter changes
                    }
                  },
                }}
              >
                <IconButton
                  type={filterOptions.size > 0 ? 'primary' : undefined}
                  data-testid="mods-filter"
                >
                  <FontAwesomeIcon icon={faFilter} />
                </IconButton>
              </DropdownModal>
              <IconButton
                data-testid="mods-view-toggle"
                onClick={() => handleViewModeChange(viewMode === 'grid' ? 'list' : 'grid')}
              >
                <FontAwesomeIcon icon={viewMode === 'grid' ? faList : faGripVertical} />
              </IconButton>
            </SearchFilterContainer>
          )}
          {/* The bar and the list it acts on, in one block, because the bar is
              sticky and sticks for as long as its containing block is on
              screen. ModsContainer spans the featured section as well, so
              without this the bar would ride on over a list it says nothing
              about.
              The block also holds the list still as the bar comes and goes:
              without that, checking the first mod pushes the whole list down by
              the height of the bar, starting with the mod under the pointer that
              checked it. */}
          <ContentHeldStill headShown={selectedIds.size > 0}>
            {/* Against the list rather than above the search row, where
                UpdatesAvailableBar sits: that bar counts every installed mod and
                below the search row would read as scoped to what the search
                left. This one is scoped to exactly that. */}
            <ModSelectionBar
              selectedCount={selectedIds.size}
              targets={selectionTargets}
              allSelected={
                selectedIds.size === installedModsFilteredAndSorted.length
              }
              busy={enableModPending || deleteModPending}
              onEnable={() => handleSelectionEnable(true)}
              onDisable={() => handleSelectionEnable(false)}
              onMoveToGroup={() => setMoveModalOpen(true)}
              onRemove={handleSelectionRemove}
              onSelectAll={selectAll}
              onClear={clearSelection}
            />
            {noInstalledMods ? (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={t('home.installedMods.noMods')}
              >
                <Button type="primary" onClick={() => navigate('/mods-browser')}>
                  {t('home.browse')}
                </Button>
              </Empty>
            ) : noFilteredResults ? (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={t('modSearch.noResults')}
              />
            ) : (
              <BlocksContainer
                data-testid="installed-mods"
                // Read past the blocks, by the CSS that draws what is in them:
                // once anything is selected every checkbox stays out, so
                // extending a selection is not a hunt and the bar's count has
                // something on screen to match. Above every block rather than on
                // each one, since a group's own header stands between two of
                // them, and its select box needs the same predicate as a card's.
                // Held here rather than passed to each card, which would
                // re-render the whole grid the first time anything was checked.
                data-selection-active={selectedIds.size > 0 ? '' : undefined}
              >
                {orderedBlocks.map((block) => {
                  const group = block.group;
                  if (!group) {
                    return block.modIds.length > 0 ? (
                      <Fragment key="ungrouped">
                        {renderBlockMods(block)}
                      </Fragment>
                    ) : null;
                  }

                  // Left out of the drawn list, the filter having hidden every
                  // mod this group holds.
                  const drawnIndex = drawnGroupIds.indexOf(group.id);
                  if (drawnIndex === -1) {
                    return null;
                  }

                  const collapsed = groupIsCollapsed(group);

                  // Read off what the block lists rather than off what the group
                  // holds, so the box never reaches a mod a filter has hidden -
                  // and a folded block still says how much of it is selected,
                  // which is what makes a selection legible from outside it.
                  const selectedInBlock = block.modIds.filter((modId) =>
                    selectedIds.has(modId)
                  ).length;

                  return (
                    <GroupBlock key={group.id} data-testid="mod-group">
                      <ModGroupHeader
                        group={group}
                        modCount={block.modIds.length}
                        collapsed={collapsed}
                        selection={
                          selectedInBlock === 0
                            ? 'none'
                            : selectedInBlock === block.modIds.length
                            ? 'all'
                            : 'some'
                        }
                        canMoveUp={drawnIndex > 0}
                        canMoveDown={drawnIndex < drawnGroupIds.length - 1}
                        onToggleCollapsed={() => toggleGroupCollapsed(group)}
                        onSelectionChange={(selected) =>
                          setBlockSelected(block.modIds, selected)
                        }
                        onMove={(delta) =>
                          swapModGroups(group.id, drawnGroupIds[drawnIndex + delta])
                        }
                        onRename={() => setRenameGroupId(group.id)}
                        onDelete={() =>
                          handleDeleteGroup(group.id, block.modIds.length)
                        }
                      />
                      {!collapsed &&
                        (block.modIds.length > 0 ? (
                          renderBlockMods(block)
                        ) : (
                          <EmptyGroupLine data-testid="mod-group-empty">
                            {t('modGroups.empty')}
                          </EmptyGroupLine>
                        ))}
                    </GroupBlock>
                  );
                })}
              </BlocksContainer>
            )}
          </ContentHeldStill>
          <SectionHeader>
            <h2>
              <SectionIcon icon={faStar} /> {t('home.featuredMods.title')}
            </h2>
          </SectionHeader>
          {featuredModsFilteredAndSorted === undefined ? (
            <ProgressSpin size="large" tip={t('general.status.loading')} />
          ) : featuredModsFilteredAndSorted === null ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t('general.status.loadingFailed')}
            >
              <Button type="primary" onClick={() => navigate('/mods-browser')}>
                {t('home.browse')}
              </Button>
            </Empty>
          ) : featuredModsFilteredAndSorted.length === 0 ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t('home.featuredMods.noMods')}
            >
              <Button type="primary" onClick={() => navigate('/mods-browser')}>
                {t('home.browse')}
              </Button>
            </Empty>
          ) : (
            <ModsGrid data-testid="featured-mods">
              {featuredModsFilteredAndSorted.map(([modId, mod]) => (
                <ModCard
                  key={modId}
                  modId={modId}
                  // No ribbon: what is featured here is what is not on the
                  // machine, so there is no installed state to report on one.
                  title={mod.metadata.name || modId}
                  description={mod.metadata.description}
                  modMetadata={mod.metadata}
                  repositoryDetails={mod.details}
                  buttons={[
                    {
                      type: 'action',
                      text: t('mod.details'),
                      testId: 'mod-card-details',
                      onClick: () => {
                        setDetailsButtonClicked(true);
                        navigate('/mods/featured/' + modId);
                      },
                    },
                  ]}
                />
              ))}
              <ExploreModsButton
                size="large"
                onClick={() => navigate('/mods-browser')}
              >
                {t('home.featuredMods.explore')}
              </ExploreModsButton>
            </ModsGrid>
          )}
        </ModsContainer>
      </ContentWrapper>
      {displayedModId && (
        <ContentWrapper>
          <ModDetails
            modId={displayedModId}
            // The catalog side of a mod opened out of the featured strip, which
            // is the only mod here this screen has one for.
            repositoryModDetails={
              showsInstalledMod ? undefined : featuredMods?.[displayedModId]
            }
            goBack={() => {
              // If we ever clicked on Details, go back.
              // Otherwise, we probably arrived from a different location,
              // go straight to the mods page.
              if (detailsButtonClicked) {
                navigate(-1);
              } else {
                navigate('/');
              }
            }}
            extensionProps={{
              installedModDetails: installedMods[displayedModId],
              // For a mod off the featured strip the repository is the whole of
              // what there is to show. For one on the machine it is read
              // wherever the repository holds a version other than the installed
              // one, whether the offer of it stands or was turned down, so the
              // version list keeps the latest version either way - and not for a
              // mod that is up to date. Never for a local mod, which has no
              // repository side at all.
              loadRepositoryData:
                !isLocalModId(displayedModId) &&
                (!showsInstalledMod ||
                  compareToRepository(
                    installedMods[displayedModId].metadata?.version,
                    installedMods[displayedModId].latestVersion
                  ).kind === 'offered'),
              // Every one of them, the install included: whether a mod that is
              // already on the machine offers one is the header's answer from
              // that fact, not this screen's from which list it was opened out
              // of.
              actions: {
                installMod: (modSource: string) =>
                  trackModOperation(
                    { modId: displayedModId },
                    installMod({ modId: displayedModId, modSource })
                  ),
                updateMod: (modSource: string) =>
                  trackModOperation(
                    { modId: displayedModId, updating: true },
                    installMod({ modId: displayedModId, modSource })
                  ),
                forkModFromSource: (modSource: string) =>
                  forkMod({ modId: displayedModId, modSource }),
                compileMod: () =>
                  trackModOperation(
                    { modId: displayedModId },
                    compileMod({ modId: displayedModId })
                  ),
                enableMod: (enable: boolean) =>
                  enableMod({ modId: displayedModId, enable }),
                editMod: () => editMod({ modId: displayedModId }),
                forkMod: () => forkMod({ modId: displayedModId }),
                deleteMod: () => deleteMod({ modId: displayedModId }),
                updateModRating: (newRating: number) =>
                  updateModRating({ modId: displayedModId, rating: newRating }),
              },
            }}
          />
        </ContentWrapper>
      )}
      {updateWizardOpen && (
        <ModUpdateWizard
          mods={updatableMods}
          onClose={() => setUpdateWizardOpen(false)}
          onModUpdated={applyInstalledModDetails}
        />
      )}
      {/* Told about the details pane rather than left to go with it: the
          installed section is hidden behind a mod's details rather than
          unmounted, so a dialog left mounted would sit on top of the pane. */}
      {moveModalOpen && !displayedModId && (
        <ModGroupMoveModal
          modIds={selectedModIds}
          groups={groups}
          onMove={handleMoveToGroup}
          onClose={() => setMoveModalOpen(false)}
        />
      )}
      {renamedGroup && !displayedModId && (
        <ModGroupRenameModal
          group={renamedGroup}
          groups={groups}
          onRename={(name) => renameModGroup(renamedGroup.id, name)}
          onClose={() => setRenameGroupId(null)}
        />
      )}
      <ModOperationModal
        installModPending={installModPending}
        compileModPending={compileModPending}
        operation={modOperation}
        onCancel={cancelModOperation}
      />
    </>
  );
}

export default ModsBrowserLocal;
