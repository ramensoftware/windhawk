import { faCaretDown, faFilter, faGripVertical, faHdd, faList, faSearch, faStar } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Badge, Button, Empty, Modal, Spin, Switch, Table, Tag, Tooltip } from 'antd';
import { ItemType } from 'antd/lib/menu/hooks/useItems';
import { produce } from 'immer';
import { useCallback, useContext, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useBlocker, useNavigate, useParams } from 'react-router-dom';
import styled, { css } from 'styled-components';
import { AppUISettingsContext } from '../appUISettings';
import EllipsisText from '../components/EllipsisText';
import { DropdownModal, dropdownModalDismissed, InputWithContextMenu } from '../components/InputWithContextMenu';
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
} from '../webviewIPC';
import {
  ModConfig,
  ModMetadata,
  RepositoryDetails,
} from '../webviewIPCMessages';
import localModIcon from './assets/local-mod-icon.svg';
import {
  mockModsBrowserLocalFeaturedMods,
  mockModsBrowserLocalInitialMods,
} from './mockData';
import ModCard from './ModCard';
import ModDetails from './ModDetails';

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

const ModNameLink = styled.a`
  color: var(--vscode-textLink-foreground, #3794ff);

  &:hover {
    color: var(--vscode-textLink-activeForeground, #4daafc);
  }
`;

const TableActionsButton = styled(Button)`
  padding: 0 6px;
  height: 22px;
`;

const ModLocalIcon = styled.img`
  height: 20px;
  margin-inline-start: 8px;
  cursor: help;
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
  > | null>(mockModsBrowserLocalInitialMods);

  const [featuredMods, setFeaturedMods] = useState<
    Record<string, FeaturedModDetailsType> | undefined | null
  >(mockModsBrowserLocalFeaturedMods || undefined);

  const [filterText, setFilterText] = useState('');
  const [filterOptions, setFilterOptions] = useState<Set<string>>(new Set());
  const [filterDropdownOpen, setFilterDropdownOpen] = useState(false);
  const [confirmModalOpen, setConfirmModalOpen] = useState(false);
  const [viewMode, setViewMode] = useState<'grid' | 'list'>(() => {
    try {
      const saved = localStorage.getItem('modsBrowserViewMode');
      return saved === 'list' ? 'list' : 'grid';
    } catch {
      return 'grid';
    }
  });

  const handleViewModeChange = useCallback((mode: 'grid' | 'list') => {
    setViewMode(mode);
    try {
      localStorage.setItem('modsBrowserViewMode', mode);
    } catch {
      // Ignore localStorage errors
    }
  }, []);

  const installedModsFilteredAndSorted = useMemo(() => {
    if (!installedMods) {
      return installedMods;
    }

    const filterWords = filterText.toLowerCase().split(/\s+/)
      .map(word => word.trim())
      .filter(word => word.length > 0);
    return Object.entries(installedMods)
      .filter(([modId, mod]) => {
        // Apply text filter
        if (filterWords.length > 0) {
          const textMatch = filterWords.every((filterWord) => {
            return (
              modId.toLowerCase().includes(filterWord) ||
              mod.metadata?.name?.toLowerCase().includes(filterWord) ||
              mod.metadata?.description?.toLowerCase().includes(filterWord)
            );
          });
          if (!textMatch) {
            return false;
          }
        }

        // Apply category filters - if none selected, show all
        if (filterOptions.size === 0) {
          return true;
        }

        // Use AND logic - mod must match ALL selected filters
        if (filterOptions.has('enabled')) {
          if (!mod.config || mod.config.disabled) {
            return false;
          }
        }

        if (filterOptions.has('disabled')) {
          if (mod.config && !mod.config.disabled) {
            return false;
          }
        }

        if (filterOptions.has('update-available')) {
          if (!mod.updateAvailable) {
            return false;
          }
        }

        return true;
      })
      .sort((a, b) => {
        const [modIdA, modA] = a;
        const [modIdB, modB] = b;
        const modAIsLocal = modIdA.startsWith('local@');
        const modBIsLocal = modIdB.startsWith('local@');

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
  }, [installedMods, filterText, filterOptions]);

  const featuredModsShuffled = useMemo(() => {
    if (!featuredMods) {
      return featuredMods;
    }

    // https://stackoverflow.com/a/6274381
    /**
     * Shuffles array in place. ES6 version
     * @param {Array} a items An array containing the items.
     */
    const shuffleArray = <T,>(a: T[]): T[] => {
      for (let i = a.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [a[i], a[j]] = [a[j], a[i]];
      }
      return a;
    };

    return shuffleArray(Object.entries(featuredMods));
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

  useUpdateInstalledModsDetails(
    useCallback(
      (data) => {
        if (installedMods) {
          const installedModsDetails = data.details;
          setInstalledMods(
            produce(installedMods, (draft) => {
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
        }
      },
      [installedMods]
    )
  );

  useSetNewModConfig(
    useCallback(
      (data) => {
        const { modId, config: newConfig } = data;
        if (installedMods) {
          setInstalledMods(
            produce(installedMods, (draft) => {
              if (draft[modId]?.config) {
                draft[modId].config = {
                  ...draft[modId].config,
                  ...newConfig,
                };
              }
            })
          );
        }
      },
      [installedMods]
    )
  );

  const { installMod, installModPending, installModContext } = useInstallMod<{
    updating: boolean;
  }>(
    useCallback(
      (data) => {
        const { modId, installedModDetails } = data;
        if (installedModDetails && installedMods) {
          setInstalledMods(
            produce(installedMods, (draft) => {
              const { metadata, config } = installedModDetails;
              draft[modId] = draft[modId] || {};
              draft[modId].metadata = metadata;
              draft[modId].config = config;
              draft[modId].updateAvailable = false;
            })
          );
        }
      },
      [installedMods]
    )
  );

  const { compileMod, compileModPending } = useCompileMod(
    useCallback(
      (data) => {
        const { modId, compiledModDetails } = data;
        if (compiledModDetails && installedMods) {
          setInstalledMods(
            produce(installedMods, (draft) => {
              const { metadata, config } = compiledModDetails;
              draft[modId] = draft[modId] || {};
              draft[modId].metadata = metadata;
              draft[modId].config = config;
              draft[modId].updateAvailable = false;
            })
          );
        }
      },
      [installedMods]
    )
  );

  const { enableMod } = useEnableMod(
    useCallback(
      (data) => {
        if (data.succeeded && installedMods) {
          const modId = data.modId;
          setInstalledMods(
            produce(installedMods, (draft) => {
              const config = draft[modId].config;
              if (config) {
                config.disabled = !data.enabled;
              }
            })
          );
        }
      },
      [installedMods]
    )
  );

  const { deleteMod } = useDeleteMod(
    useCallback(
      (data) => {
        if (data.succeeded && installedMods) {
          const modId = data.modId;

          if (displayedModType === 'local' && displayedModId === modId) {
            navigate('/', { replace: true });
          }

          setInstalledMods(
            produce(installedMods, (draft) => {
              delete draft[modId];
            })
          );
        }
      },
      [displayedModId, displayedModType, installedMods, navigate]
    )
  );

  const { updateModRating } = useUpdateModRating(
    useCallback(
      (data) => {
        if (data.succeeded && installedMods) {
          const modId = data.modId;
          setInstalledMods(
            produce(installedMods, (draft) => {
              draft[modId].userRating = data.rating;
            })
          );
        }
      },
      [installedMods]
    )
  );

  const [detailsButtonClicked, setDetailsButtonClicked] = useState(false);

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

  // Block all navigation when modal is open
  const modalIsOpen = installModPending || compileModPending || confirmModalOpen;

  useBlocker(({ currentLocation, nextLocation }) => {
    return modalIsOpen && currentLocation.pathname !== nextLocation.pathname;
  });

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
          {!noInstalledMods && (
            <SearchFilterContainer>
              <SearchFilterInput
                prefix={<FontAwesomeIcon icon={faSearch} />}
                placeholder={t('modSearch.placeholder') as string}
                allowClear
                value={filterText}
                onChange={(e) => setFilterText(e.target.value)}
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
                      dropdownModalDismissed();
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
                >
                  <FontAwesomeIcon icon={faFilter} />
                </IconButton>
              </DropdownModal>
              <IconButton
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
            <ModsGrid>
              {installedModsFilteredAndSorted.map(([modId, mod]) => (
                <ModCard
                  key={modId}
                  ribbonText={
                    mod.updateAvailable
                      ? (t('mod.updateAvailable') as string)
                      : undefined
                  }
                  title={mod.metadata?.name || modId.replace(/^local@/, '')}
                  isLocal={modId.startsWith('local@')}
                  description={mod.metadata?.description}
                  buttons={[
                    {
                      text: t('mod.details'),
                      onClick: () => {
                        setDetailsButtonClicked(true);
                        navigate('/mods/local/' + modId);
                      },
                      badge: (mod.config?.loggingEnabled || mod.config?.debugLoggingEnabled) ? {
                        tooltip: t('mod.loggingEnabledInAdvancedTab') as string,
                      } : undefined,
                    },
                    {
                      text: t('mod.remove'),
                      confirmText: t('mod.removeConfirm') as string,
                      confirmOkText: t('mod.removeConfirmOk') as string,
                      confirmCancelText: t('mod.removeConfirmCancel') as string,
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
            <Table
              bordered
              dataSource={installedModsFilteredAndSorted.map(([modId, mod]) => ({
                key: modId,
                modId,
                name: mod.metadata?.name || modId.replace(/^local@/, ''),
                description: mod.metadata?.description,
                author: mod.metadata?.author,
                version: mod.metadata?.version,
                isLocal: modId.startsWith('local@'),
                updateAvailable: mod.updateAvailable,
                disabled: mod.config ? mod.config.disabled : true,
                notCompiled: !mod.config,
                mod,
              }))}
              columns={[
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
                          dropdownModalDismissed();
                          compileMod({ modId: record.modId });
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
                          dropdownModalDismissed();
                          enableMod({ modId: record.modId, enable: record.disabled });
                        },
                      });
                    }

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
                          dropdownModalDismissed();
                          editMod({ modId: record.modId });
                        },
                      });
                    }

                    // Fork action
                    menuItems.push({
                      label: t('mod.fork'),
                      key: 'fork',
                      onClick: () => {
                        dropdownModalDismissed();
                        forkMod({ modId: record.modId });
                      },
                    });

                    // Divider before remove
                    menuItems.push({ type: 'divider' });

                    // Remove action
                    menuItems.push({
                      label: t('mod.remove'),
                      key: 'remove',
                      danger: true,
                      onClick: () => {
                        dropdownModalDismissed();
                        setConfirmModalOpen(true);
                        Modal.confirm({
                          title: t('mod.removeConfirm'),
                          okText: t('mod.removeConfirmOk'),
                          cancelText: t('mod.removeConfirmCancel'),
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
                    <>
                      <ModNameLink
                        onClick={() => {
                          setDetailsButtonClicked(true);
                          navigate('/mods/local/' + record.modId);
                        }}
                      >
                        {name}
                      </ModNameLink>
                      {record.updateAvailable && (
                        <Tag color="warning" style={{ marginInlineStart: 8, userSelect: 'none' }}>
                          {t('mod.updateAvailable')}
                        </Tag>
                      )}
                      {record.isLocal && (
                        <Tooltip title={t('mod.editedLocally')} placement="bottom">
                          <ModLocalIcon src={localModIcon} />
                        </Tooltip>
                      )}
                    </>
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
              ]}
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
            <ProgressSpin size="large" tip={t('general.loading')} />
          ) : featuredModsFilteredAndSorted === null ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t('general.loadingFailed')}
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
            <ModsGrid>
              {featuredModsFilteredAndSorted.map(([modId, mod]) => (
                <ModCard
                  key={modId}
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
                      text: t('mod.details'),
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
              installedModDetails={installedMods[displayedModId]}
              loadRepositoryData={installedMods[displayedModId].updateAvailable}
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
              updateMod={(modSource, disabled) =>
                installMod(
                  { modId: displayedModId, modSource, disabled },
                  { updating: true }
                )
              }
              forkModFromSource={(modSource) =>
                forkMod({ modId: displayedModId, modSource })
              }
              compileMod={() => compileMod({ modId: displayedModId })}
              enableMod={(enable) =>
                enableMod({ modId: displayedModId, enable })
              }
              editMod={() => editMod({ modId: displayedModId })}
              forkMod={() => forkMod({ modId: displayedModId })}
              deleteMod={() => deleteMod({ modId: displayedModId })}
              updateModRating={(newRating) =>
                updateModRating({ modId: displayedModId, rating: newRating })
              }
            />
          ) : (
            <ModDetails
              modId={displayedModId}
              installedModDetails={installedMods[displayedModId]}
              repositoryModDetails={featuredMods?.[displayedModId]}
              loadRepositoryData={!displayedModId.startsWith('local@')}
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
              installMod={(modSource) =>
                installMod({ modId: displayedModId, modSource: modSource })
              }
              updateMod={(modSource, disabled) =>
                installMod(
                  { modId: displayedModId, modSource, disabled },
                  { updating: true }
                )
              }
              forkModFromSource={(modSource) =>
                forkMod({ modId: displayedModId, modSource })
              }
              compileMod={() => compileMod({ modId: displayedModId })}
              enableMod={(enable) =>
                enableMod({ modId: displayedModId, enable })
              }
              editMod={() => editMod({ modId: displayedModId })}
              forkMod={() => forkMod({ modId: displayedModId })}
              deleteMod={() => deleteMod({ modId: displayedModId })}
              updateModRating={(newRating) =>
                updateModRating({ modId: displayedModId, rating: newRating })
              }
            />
          )}
        </ContentWrapper>
      )}
      {(installModPending || compileModPending) && (
        <Modal open={true} closable={false} footer={null}>
          <ProgressSpin
            size="large"
            tip={
              installModPending
                ? installModContext?.updating
                  ? t('general.updating')
                  : t('general.installing')
                : compileModPending
                  ? t('general.compiling')
                  : ''
            }
          />
        </Modal>
      )}
    </>
  );
}

export default ModsBrowserLocal;
