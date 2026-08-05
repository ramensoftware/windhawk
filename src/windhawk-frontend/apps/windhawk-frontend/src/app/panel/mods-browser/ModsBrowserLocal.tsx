import { AppUISettingsContext } from '@app/appUISettings';
import EllipsisText from '@app/components/EllipsisText';
import { DropdownModal, InputWithContextMenu } from '@app/components/InputWithContextMenu';
import { useNavigationBlock } from '@app/navigationBlock';
import { getDisplayModId, isLocalModId, isMobile, shuffleArray } from '@app/utils';
import {
  editMod,
  forkMod,
  useCompileMod,
  useDeleteMod,
  useEnableMod,
  useGetFeaturedMods,
  useGetInstalledMods,
  useInstallMod,
  useSetNewModConfig,
  useUpdateInstalledModsDetails,
  useUpdateModRating,
} from '@app/webviewIPC';
import {
  type ModConfig,
  type ModMetadata,
  type RepositoryDetails,
} from '@app/webviewIPCMessages';
import { faCaretDown, faFilter, faGripVertical, faHdd, faList, faSearch, faStar } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Badge, Button, Empty, type InputRef, Modal, Spin, Switch, Table, Tag, Tooltip } from 'antd';
import { type ItemType } from 'antd/lib/menu/hooks/useItems';
import { type ColumnsType } from 'antd/lib/table';
import { produce } from 'immer';
import { useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';
import styled, { css } from 'styled-components';
import { ModDetails } from '../mod-details';
import { ModCard } from '../shared';
import LocalModIcon from '../shared/LocalModIcon';
import useKeyboardShortcut, { isTypingTarget } from '../shared/useKeyboardShortcut';
import ModOperationModal from './ModOperationModal';
import { ModUpdateWizard, UpdatesAvailableBar } from './update-wizard';
import {
  type ModOperationContext,
  useCancelModOperation,
} from './useCancelModOperation';

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

// antd's per-cell border-right (the column separators) is dropped in production
// by a cssnano :is()-folding bug (https://github.com/cssnano/cssnano/issues/1786,
// fixed in cssnano 8.0.1; this build ships 7.1.x); redeclaring via
// styled-components bypasses the minifier. The right edge sits on the container
// rather than on the last cell, whose border Chromium drops under
// table-layout: fixed; clearing that cell's border keeps the edge a single line
// where antd's rule does survive the minifier.
// --whui-border matches antd's table border color per theme.
const ModsTable = styled(Table)`
  .ant-table.ant-table-bordered .ant-table-cell:not(:last-child) {
    border-right: 1px solid var(--whui-border);
  }

  .ant-table.ant-table-bordered .ant-table-cell:last-child {
    border-right: 0;
  }

  .ant-table.ant-table-bordered > .ant-table-container {
    border-right: 1px solid var(--whui-border);
  }
` as typeof Table;

const TableActionsButton = styled(Button)`
  padding: 0 6px;
  height: 22px;
`;

const ModNameCellContent = styled.span`
  display: inline-flex;
  gap: 8px;
  flex-wrap: wrap;
  align-items: center;
`;

const ModNameLink = styled.a`
  color: var(--whui-link);

  &:hover {
    color: var(--whui-link-hover);
  }
`;

const ModLocalIcon = styled(LocalModIcon)`
  width: 20px;
  height: 20px;
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

type ModDetailsType = {
  metadata: ModMetadata | null;
  config: ModConfig | null;
  updateAvailable: boolean;
  userRating: number;
};

type FeaturedModDetailsType = {
  metadata: ModMetadata;
  details: RepositoryDetails;
};

type ModTableRow = {
  key: string;
  modId: string;
  name: string;
  description?: string;
  author?: string;
  version?: string;
  isLocal: boolean;
  updateAvailable: boolean;
  disabled: boolean;
  notCompiled: boolean;
  mod: ModDetailsType;
};

function modMatchesFilter(
  modId: string,
  mod: ModDetailsType,
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

  if (filterOptions.has('update-available') && !mod.updateAvailable) {
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

  const [installedMods, setInstalledMods] = useState<Record<
    string,
    ModDetailsType
  > | null>(null);

  const [featuredMods, setFeaturedMods] = useState<
    Record<string, FeaturedModDetailsType> | undefined | null
  >(undefined);

  const [filterText, setFilterText] = useState('');
  const [filterOptions, setFilterOptions] = useState<Set<string>>(new Set());
  const [filterDropdownOpen, setFilterDropdownOpen] = useState(false);
  const [confirmModalOpen, setConfirmModalOpen] = useState(false);
  const [updateWizardOpen, setUpdateWizardOpen] = useState(false);
  const [viewMode, setViewMode] = useState<'grid' | 'list'>(() => {
    try {
      const saved = localStorage.getItem('modsBrowserViewMode');
      return saved === 'list' ? 'list' : 'grid';
    } catch {
      return 'grid';
    }
  });
  const [searchInputFocused, setSearchInputFocused] = useState(!isMobile);

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
    localStorage.setItem('modsBrowserViewMode', mode);
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

  // The mods an update is waiting for, read off every installed mod rather than
  // off installedModsFilteredAndSorted: the bar and the wizard describe the
  // machine, not the search box, so a user who has typed a filter still gets the
  // true count. Local mods are excluded - they have no repository counterpart, so
  // a stale flag on one would produce a row whose update has no source.
  const updatableMods = useMemo(
    () =>
      Object.entries(installedMods ?? {})
        .filter(([modId, mod]) => mod.updateAvailable && !isLocalModId(modId))
        .map(([modId, mod]) => ({
          modId,
          name: mod.metadata?.name || getDisplayModId(modId),
          installedVersion: mod.metadata?.version,
        })),
    [installedMods]
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

  const { getInstalledMods } = useGetInstalledMods(
    useCallback((data) => {
      setInstalledMods(data.installedMods);
    }, [])
  );

  const { getFeaturedMods } = useGetFeaturedMods(
    useCallback((data) => {
      setFeaturedMods(data.featuredMods);
    }, [])
  );

  useEffect(() => {
    getInstalledMods({});
    getFeaturedMods({});
  }, [getInstalledMods, getFeaturedMods]);

  // Local mods can change outside the webview; refresh the installed mods list
  // when the window regains focus. Local-only: does not fetch featured mods or
  // touch the network.
  useEffect(() => {
    const handleWindowFocus = () => {
      getInstalledMods({});
    };
    window.addEventListener('focus', handleWindowFocus);
    return () => {
      window.removeEventListener('focus', handleWindowFocus);
    };
  }, [getInstalledMods]);

  useUpdateInstalledModsDetails(
    useCallback((data) => {
      const installedModsDetails = data.details;
      setInstalledMods((prev) =>
        prev &&
        produce(prev, (draft) => {
          for (const [modId, updatedDetails] of Object.entries(
            installedModsDetails
          )) {
            const details = draft[modId];
            if (details) {
              const { updateAvailable, userRating } = updatedDetails;
              details.updateAvailable = updateAvailable;
              details.userRating = userRating;
            }
          }
        })
      );
    }, [])
  );

  useSetNewModConfig(
    useCallback((data) => {
      const { modId, config: newConfig } = data;
      setInstalledMods((prev) =>
        prev &&
        produce(prev, (draft) => {
          if (draft[modId]?.config) {
            draft[modId].config = {
              ...draft[modId].config,
              ...newConfig,
            };
          }
        })
      );
    }, [])
  );

  const { installMod, installModPending, installModContext } = useInstallMod<ModOperationContext>(
    useCallback((data) => {
      const { modId, installedModDetails } = data;
      if (!installedModDetails) {
        return;
      }
      setInstalledMods((prev) =>
        prev &&
        produce(prev, (draft) => {
          const { metadata, config } = installedModDetails;
          draft[modId] = draft[modId] || {};
          draft[modId].metadata = metadata;
          draft[modId].config = config;
          draft[modId].updateAvailable = false;
        })
      );
    }, [])
  );

  const { compileMod, compileModPending, compileModContext } = useCompileMod<ModOperationContext>(
    useCallback((data) => {
      const { modId, compiledModDetails } = data;
      if (!compiledModDetails) {
        return;
      }
      setInstalledMods((prev) =>
        prev &&
        produce(prev, (draft) => {
          const { metadata, config } = compiledModDetails;
          draft[modId] = draft[modId] || {};
          draft[modId].metadata = metadata;
          draft[modId].config = config;
          draft[modId].updateAvailable = false;
        })
      );
    }, [])
  );

  const cancelModOperation = useCancelModOperation({
    installModPending,
    installModContext,
    compileModPending,
    compileModContext,
  });

  const { enableMod } = useEnableMod(
    useCallback((data) => {
      if (!data.succeeded) {
        return;
      }
      const modId = data.modId;
      setInstalledMods((prev) =>
        prev &&
        produce(prev, (draft) => {
          const config = draft[modId].config;
          if (config) {
            config.disabled = !data.enabled;
          }
        })
      );
    }, [])
  );

  const { deleteMod } = useDeleteMod(
    useCallback(
      (data) => {
        if (!data.succeeded) {
          return;
        }
        const modId = data.modId;

        if (displayedModType === 'local' && displayedModId === modId) {
          navigate('/', { replace: true });
        }

        setInstalledMods((prev) =>
          prev &&
          produce(prev, (draft) => {
            delete draft[modId];
          })
        );
      },
      [displayedModId, displayedModType, navigate]
    )
  );

  const { updateModRating } = useUpdateModRating(
    useCallback((data) => {
      if (!data.succeeded) {
        return;
      }
      const modId = data.modId;
      setInstalledMods((prev) =>
        prev &&
        produce(prev, (draft) => {
          draft[modId].userRating = data.rating;
        })
      );
    }, [])
  );

  const [detailsButtonClicked, setDetailsButtonClicked] = useState(false);

  const tableDataSource = useMemo<ModTableRow[]>(
    () =>
      (installedModsFilteredAndSorted ?? []).map(([modId, mod]) => ({
        key: modId,
        modId,
        name: mod.metadata?.name || getDisplayModId(modId),
        description: mod.metadata?.description,
        author: mod.metadata?.author,
        version: mod.metadata?.version,
        isLocal: isLocalModId(modId),
        updateAvailable: mod.updateAvailable,
        disabled: mod.config ? mod.config.disabled : true,
        notCompiled: !mod.config,
        mod,
      })),
    [installedModsFilteredAndSorted]
  );

  const tableColumns = useMemo<ColumnsType<ModTableRow>>(
    () => [
      {
        title: '',
        key: 'actions',
        width: 50,
        align: 'center',
        render: (_, record) => {
          const isLocal = record.isLocal;
          const menuItems: ItemType[] = [];

          // Compile action (if not compiled)
          if (record.notCompiled) {
            menuItems.push({
              label: t('mod.compile'),
              key: 'compile',
              onClick: () => {
                compileMod({ modId: record.modId }, { modId: record.modId });
              },
            });
          }

          // Enable/Disable action (if compiled)
          if (!record.notCompiled) {
            menuItems.push({
              label: record.disabled
                ? t('mod.enable')
                : t('mod.disable'),
              key: 'toggle-enable',
              onClick: () => {
                enableMod({ modId: record.modId, enable: record.disabled });
              },
            });
          }

          // Dev actions (only when development mode is enabled)
          if (!devModeOptOut) {
            // Divider before dev actions
            if (menuItems.length > 0) {
              menuItems.push({ type: 'divider' });
            }

            // Edit action (local mods only)
            if (isLocal) {
              menuItems.push({
                label: t('mod.edit'),
                key: 'edit',
                onClick: () => {
                  editMod({ modId: record.modId });
                },
              });
            }

            // Fork action
            menuItems.push({
              label: t('mod.fork'),
              key: 'fork',
              onClick: () => {
                forkMod({ modId: record.modId });
              },
            });
          }

          // Divider before remove
          menuItems.push({ type: 'divider' });

          // Remove action
          menuItems.push({
            label: t('mod.remove'),
            key: 'remove',
            danger: true,
            onClick: () => {
              setConfirmModalOpen(true);
              Modal.confirm({
                title: t('mod.removeConfirm'),
                okText: t('mod.removeConfirmOk'),
                cancelText: t('general.actions.cancel'),
                okButtonProps: { danger: true },
                onOk: () => {
                  setConfirmModalOpen(false);
                  deleteMod({ modId: record.modId });
                },
                onCancel: () => {
                  setConfirmModalOpen(false);
                },
                closable: true,
                maskClosable: true,
              });
            },
          });

          const hasLogging = record.mod.config?.loggingEnabled || record.mod.config?.debugLoggingEnabled;
          const actionsButton = (
            <DropdownModal
              menu={{ items: menuItems }}
              trigger={['click']}
            >
              <TableActionsButton>
                <FontAwesomeIcon icon={faCaretDown} />
              </TableActionsButton>
            </DropdownModal>
          );

          if (hasLogging) {
            return (
              <Badge
                dot
                title={t('mod.loggingEnabledInAdvancedTab') as string}
                status="warning"
              >
                {actionsButton}
              </Badge>
            );
          }

          return actionsButton;
        },
      },
      {
        title: t('home.installedMods.grid.name'),
        dataIndex: 'name',
        key: 'name',
        width: '30%',
        sorter: (a, b) => a.name.localeCompare(b.name),
        render: (name, record) => (
          <ModNameCellContent>
            <ModNameLink
              onClick={() => {
                setDetailsButtonClicked(true);
                navigate('/mods/local/' + record.modId);
              }}
            >
              {name}
            </ModNameLink>
            {record.updateAvailable && (
              <Tag color="warning" style={{ margin: 0, userSelect: 'none' }}>
                {t('mod.updateAvailable')}
              </Tag>
            )}
            {record.isLocal && (
              <Tooltip title={t('mod.editedLocally')} placement="bottom">
                <ModLocalIcon aria-label={t('mod.editedLocally')} />
              </Tooltip>
            )}
          </ModNameCellContent>
        ),
      },
      {
        title: t('home.installedMods.grid.description'),
        dataIndex: 'description',
        key: 'description',
        render: (description) => (
          <EllipsisText tooltipPlacement="bottom">{description || '-'}</EllipsisText>
        ),
        ellipsis: { showTitle: false },
      },
      {
        title: t('home.installedMods.grid.author'),
        dataIndex: 'author',
        key: 'author',
        width: '12%',
        sorter: (a, b) => (a.author || '').localeCompare(b.author || ''),
        render: (author) => author || '-',
      },
      {
        title: t('home.installedMods.grid.version'),
        dataIndex: 'version',
        key: 'version',
        width: '8%',
        sorter: (a, b) => {
          const versionA = a.version || '';
          const versionB = b.version || '';
          return versionA.localeCompare(versionB, undefined, { numeric: true, sensitivity: 'base' });
        },
        render: (version) => version || '-',
      },
      {
        title: t('home.installedMods.grid.status'),
        key: 'status',
        width: 80,
        align: 'center',
        sorter: (a, b) => Number(a.disabled) - Number(b.disabled),
        render: (_, record) => (
          <Switch
            checked={!record.disabled}
            disabled={record.notCompiled}
            onChange={(checked) =>
              enableMod({ modId: record.modId, enable: checked })
            }
            title={
              record.notCompiled
                ? (t('mod.notCompiled') as string)
                : undefined
            }
          />
        ),
      },
    ],
    [t, devModeOptOut, navigate, compileMod, enableMod, deleteMod]
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

  if (!installedMods || !installedModsFilteredAndSorted) {
    return null;
  }

  const noInstalledMods = Object.keys(installedMods).length === 0;
  const noFilteredResults = installedModsFilteredAndSorted.length === 0 && !noInstalledMods;

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
          ) : viewMode === 'grid' ? (
            <ModsGrid data-testid="installed-mods">
              {installedModsFilteredAndSorted.map(([modId, mod]) => (
                <ModCard
                  key={modId}
                  modId={modId}
                  ribbonText={
                    mod.updateAvailable
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
                    title: mod.config
                      ? undefined
                      : (t('mod.notCompiled') as string),
                    checked: mod.config ? !mod.config.disabled : false,
                    disabled: !mod.config,
                    onChange: (checked) =>
                      enableMod({ modId, enable: checked }),
                  }}
                />
              ))}
            </ModsGrid>
          ) : (
            <ModsTable
              bordered
              data-testid="installed-mods-table"
              dataSource={tableDataSource}
              columns={tableColumns}
              pagination={false}
              size="middle"
              showSorterTooltip={false}
              style={{ wordBreak: 'break-word' }}
            />
          )}
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
                  ribbonText={
                    installedMods[modId]
                      ? installedMods[modId].metadata?.version !==
                        mod.metadata.version
                        ? (t('mod.updateAvailable') as string)
                        : (t('mod.installed') as string)
                      : undefined
                  }
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
          {(displayedModType === 'local' && installedMods[displayedModId]) ? (
            <ModDetails
              modId={displayedModId}
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
                loadRepositoryData: installedMods[displayedModId].updateAvailable,
                updateMod: (modSource: string) =>
                  installMod(
                    { modId: displayedModId, modSource },
                    { modId: displayedModId, updating: true }
                  ),
                forkModFromSource: (modSource: string) =>
                  forkMod({ modId: displayedModId, modSource }),
                compileMod: () =>
                  compileMod({ modId: displayedModId }, { modId: displayedModId }),
                enableMod: (enable: boolean) =>
                  enableMod({ modId: displayedModId, enable }),
                editMod: () => editMod({ modId: displayedModId }),
                forkMod: () => forkMod({ modId: displayedModId }),
                deleteMod: () => deleteMod({ modId: displayedModId }),
                updateModRating: (newRating: number) =>
                  updateModRating({ modId: displayedModId, rating: newRating }),
              }}
            />
          ) : (
            <ModDetails
              modId={displayedModId}
              repositoryModDetails={featuredMods?.[displayedModId]}
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
                loadRepositoryData: !isLocalModId(displayedModId),
                installMod: (modSource: string) =>
                  installMod(
                    { modId: displayedModId, modSource: modSource },
                    { modId: displayedModId }
                  ),
                updateMod: (modSource: string) =>
                  installMod(
                    { modId: displayedModId, modSource },
                    { modId: displayedModId, updating: true }
                  ),
                forkModFromSource: (modSource: string) =>
                  forkMod({ modId: displayedModId, modSource }),
                compileMod: () =>
                  compileMod({ modId: displayedModId }, { modId: displayedModId }),
                enableMod: (enable: boolean) =>
                  enableMod({ modId: displayedModId, enable }),
                editMod: () => editMod({ modId: displayedModId }),
                forkMod: () => forkMod({ modId: displayedModId }),
                deleteMod: () => deleteMod({ modId: displayedModId }),
                updateModRating: (newRating: number) =>
                  updateModRating({ modId: displayedModId, rating: newRating }),
              }}
            />
          )}
        </ContentWrapper>
      )}
      {updateWizardOpen && (
        <ModUpdateWizard
          mods={updatableMods}
          onClose={() => setUpdateWizardOpen(false)}
          onModUpdated={(modId, installedModDetails) => {
            setInstalledMods((prev) =>
              prev &&
              produce(prev, (draft) => {
                const { metadata, config } = installedModDetails;
                draft[modId] = draft[modId] || {};
                draft[modId].metadata = metadata;
                draft[modId].config = config;
                draft[modId].updateAvailable = false;
              })
            );
          }}
        />
      )}
      <ModOperationModal
        installModPending={installModPending}
        installModContext={installModContext}
        compileModPending={compileModPending}
        onCancel={cancelModOperation}
      />
    </>
  );
}

export default ModsBrowserLocal;
