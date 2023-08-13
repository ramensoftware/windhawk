import { faSearch, faSort } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, Dropdown, Modal, Result, Spin } from 'antd';
import produce from 'immer';
import { useCallback, useContext, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import InfiniteScroll from 'react-infinite-scroll-component';
import { useNavigate, useParams } from 'react-router-dom';
import styled, { css } from 'styled-components';
import { AppUISettingsContext } from '../appUISettings';
import InputWithContextMenu from '../components/InputWithContextMenu';
import {
  editMod,
  forkMod,
  useCompileMod,
  useDeleteMod,
  useEnableMod,
  useGetRepositoryMods,
  useInstallMod,
  useUpdateInstalledModsDetails,
  useUpdateModRating,
} from '../webviewIPC';
import {
  ModConfig,
  ModMetadata,
  RepositoryDetails,
} from '../webviewIPCMessages';
import { mockModsBrowserOnlineRepositoryMods, useMockData } from './mockData';
import ModCard from './ModCard';
import ModDetails from './ModDetails';

const CenteredContainer = styled.div`
  display: flex;
  flex-direction: column;
  height: 100%;
`;

const CenteredContent = styled.div`
  margin: auto;

  // Without this the centered content looks too low.
  padding-bottom: 10vh;
`;

const SearchFilterContainer = styled.div`
  display: flex;
  gap: 10px;
  margin: 20px 0;
`;

const SearchFilterInput = styled(InputWithContextMenu.Input)`
  > .ant-input-prefix {
    margin-right: 8px;
  }
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

const ProgressSpin = styled(Spin)`
  display: block;
  margin-left: auto;
  margin-right: auto;
  font-size: 32px;
`;

type ModDetailsType = {
  repository: {
    metadata: ModMetadata;
    details: RepositoryDetails;
  };
  installed?: {
    metadata: ModMetadata | null;
    config: ModConfig | null;
    userRating?: number;
  };
};

interface Props {
  ContentWrapper: React.ComponentType<
    React.ComponentPropsWithRef<'div'> & { $hidden?: boolean }
  >;
}

function ModsBrowserOnline({ ContentWrapper }: Props) {
  const { t } = useTranslation();

  const navigate = useNavigate();
  const replace = useCallback(
    (to: string) => navigate(to, { replace: true }),
    [navigate]
  );

  const { modId: displayedModId } = useParams<{ modId: string }>();

  const [initialDataPending, setInitialDataPending] = useState(true);
  const [repositoryMods, setRepositoryMods] = useState<Record<
    string,
    ModDetailsType
  > | null>(mockModsBrowserOnlineRepositoryMods);

  const [sortingOrder, setSortingOrder] = useState('popular-top-rated');
  const [filterText, setFilterText] = useState('');

  const installedModsFilteredAndSorted = useMemo(() => {
    const filterWords = filterText.trim().toLowerCase().split(/\s+/);
    return Object.entries(repositoryMods || {})
      .filter(([modId, mod]) => {
        if (filterWords.length === 0) {
          return true;
        }

        return filterWords.every((filterWord) => {
          return (
            mod.repository.metadata.name?.toLowerCase().includes(filterWord) ||
            mod.repository.metadata.description
              ?.toLowerCase()
              .includes(filterWord)
          );
        });
      })
      .sort((a, b) => {
        const [modIdA, modA] = a;
        const [modIdB, modB] = b;

        switch (sortingOrder) {
          case 'popular-top-rated':
            if (
              modB.repository.details.defaultSorting <
              modA.repository.details.defaultSorting
            ) {
              return -1;
            } else if (
              modB.repository.details.defaultSorting >
              modA.repository.details.defaultSorting
            ) {
              return 1;
            }
            break;

          case 'popular':
            if (modB.repository.details.users < modA.repository.details.users) {
              return -1;
            } else if (
              modB.repository.details.users > modA.repository.details.users
            ) {
              return 1;
            }
            break;

          case 'top-rated':
            if (
              modB.repository.details.rating < modA.repository.details.rating
            ) {
              return -1;
            } else if (
              modB.repository.details.rating > modA.repository.details.rating
            ) {
              return 1;
            }
            break;

          case 'newest':
            if (
              modB.repository.details.published <
              modA.repository.details.published
            ) {
              return -1;
            } else if (
              modB.repository.details.published >
              modA.repository.details.published
            ) {
              return 1;
            }
            break;

          case 'last-updated':
            if (
              modB.repository.details.updated < modA.repository.details.updated
            ) {
              return -1;
            } else if (
              modB.repository.details.updated > modA.repository.details.updated
            ) {
              return 1;
            }
            break;

          case 'alphabetical':
            // Nothing to do.
            break;
        }

        // Fallback sorting: Sort by name, then id.

        const modATitle = (
          modA.repository.metadata.name || modIdA
        ).toLowerCase();
        const modBTitle = (
          modB.repository.metadata.name || modIdB
        ).toLowerCase();

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
  }, [repositoryMods, sortingOrder, filterText]);

  const { devModeOptOut } = useContext(AppUISettingsContext);

  const { getRepositoryMods } = useGetRepositoryMods(
    useCallback((data) => {
      setRepositoryMods(data.mods);
      setInitialDataPending(false);
    }, [])
  );

  useEffect(() => {
    let pending = false;
    if (!useMockData) {
      getRepositoryMods({});
      pending = true;
    }

    setInitialDataPending(pending);
  }, [getRepositoryMods]);

  useUpdateInstalledModsDetails(
    useCallback(
      (data) => {
        if (repositoryMods) {
          const installedModsDetails = data.details;
          setRepositoryMods(
            produce(repositoryMods, (draft) => {
              for (const [modId, updatedDetails] of Object.entries(
                installedModsDetails
              )) {
                const details = draft[modId]?.installed;
                if (details) {
                  const { userRating } = updatedDetails;
                  details.userRating = userRating;
                }
              }
            })
          );
        }
      },
      [repositoryMods]
    )
  );

  const { installMod, installModPending, installModContext } = useInstallMod<{
    updating: boolean;
  }>(
    useCallback(
      (data) => {
        const { installedModDetails } = data;
        if (installedModDetails && repositoryMods) {
          const modId = data.modId;
          setRepositoryMods(
            produce(repositoryMods, (draft) => {
              draft[modId].installed = installedModDetails;
            })
          );
        }
      },
      [repositoryMods]
    )
  );

  const { compileMod, compileModPending } = useCompileMod(
    useCallback(
      (data) => {
        const { compiledModDetails } = data;
        if (compiledModDetails && repositoryMods) {
          const modId = data.modId;
          setRepositoryMods(
            produce(repositoryMods, (draft) => {
              draft[modId].installed = compiledModDetails;
            })
          );
        }
      },
      [repositoryMods]
    )
  );

  const { enableMod } = useEnableMod(
    useCallback(
      (data) => {
        if (data.succeeded && repositoryMods) {
          const modId = data.modId;
          setRepositoryMods(
            produce(repositoryMods, (draft) => {
              const config = draft[modId].installed?.config;
              if (config) {
                config.disabled = !data.enabled;
              }
            })
          );
        }
      },
      [repositoryMods]
    )
  );

  const { deleteMod } = useDeleteMod(
    useCallback(
      (data) => {
        if (data.succeeded && repositoryMods) {
          const modId = data.modId;
          setRepositoryMods(
            produce(repositoryMods, (draft) => {
              delete draft[modId].installed;
            })
          );
        }
      },
      [repositoryMods]
    )
  );

  const { updateModRating } = useUpdateModRating(
    useCallback(
      (data) => {
        if (data.succeeded && repositoryMods) {
          const modId = data.modId;
          setRepositoryMods(
            produce(repositoryMods, (draft) => {
              const installed = draft[modId].installed;
              if (installed) {
                installed.userRating = data.rating;
              }
            })
          );
        }
      },
      [repositoryMods]
    )
  );

  const [infiniteScrollLoadedItems, setInfiniteScrollLoadedItems] =
    useState(30);

  const resetInfiniteScrollLoadedItems = () => setInfiniteScrollLoadedItems(30);

  if (initialDataPending) {
    return (
      <CenteredContainer>
        <CenteredContent>
          <ProgressSpin size="large" tip={t('general.loading')} />
        </CenteredContent>
      </CenteredContainer>
    );
  }

  if (!repositoryMods) {
    return (
      <CenteredContainer>
        <CenteredContent>
          <Result
            status="error"
            title={t('general.loadingFailedTitle')}
            subTitle={t('general.loadingFailedSubtitle')}
            extra={[
              <Button
                type="primary"
                key="try-again"
                onClick={() => getRepositoryMods({})}
              >
                {t('general.tryAgain')}
              </Button>,
            ]}
          />
        </CenteredContent>
      </CenteredContainer>
    );
  }

  return (
    <>
      <ContentWrapper
        id="ModsBrowserOnline-ContentWrapper"
        $hidden={!!displayedModId}
      >
        <ModsContainer $extraBottomPadding={!devModeOptOut}>
          <SearchFilterContainer>
            <SearchFilterInput
              prefix={<FontAwesomeIcon icon={faSearch} />}
              placeholder={t('explore.search.placeholder') as string}
              allowClear
              value={filterText}
              onChange={(e) => {
                resetInfiniteScrollLoadedItems();
                setFilterText(e.target.value);
              }}
            />
            <Dropdown
              placement="bottomRight"
              trigger={['click']}
              arrow={true}
              menu={{
                items: [
                  {
                    label: t('explore.search.popularAndTopRated'),
                    key: 'popular-top-rated',
                  },
                  { label: t('explore.search.popular'), key: 'popular' },
                  { label: t('explore.search.topRated'), key: 'top-rated' },
                  { label: t('explore.search.newest'), key: 'newest' },
                  {
                    label: t('explore.search.lastUpdated'),
                    key: 'last-updated',
                  },
                  {
                    label: t('explore.search.alphabeticalOrder'),
                    key: 'alphabetical',
                  },
                ],
                selectedKeys: [sortingOrder],
                onClick: (e) => {
                  resetInfiniteScrollLoadedItems();
                  setSortingOrder(e.key);
                },
              }}
            >
              <Button>
                <FontAwesomeIcon icon={faSort} />
              </Button>
            </Dropdown>
          </SearchFilterContainer>
          <InfiniteScroll
            dataLength={infiniteScrollLoadedItems}
            next={() =>
              setInfiniteScrollLoadedItems(
                Math.min(
                  infiniteScrollLoadedItems + 30,
                  installedModsFilteredAndSorted.length
                )
              )
            }
            hasMore={
              infiniteScrollLoadedItems < installedModsFilteredAndSorted.length
            }
            loader={null}
            scrollableTarget="ModsBrowserOnline-ContentWrapper"
            style={{ overflow: 'visible' }} // for the ribbon
          >
            <ModsGrid>
              {installedModsFilteredAndSorted
                .slice(0, infiniteScrollLoadedItems)
                .map(([modId, mod]) => (
                  <ModCard
                    key={modId}
                    ribbonText={
                      mod.installed
                        ? mod.installed.metadata?.version !==
                          mod.repository.metadata.version
                          ? (t('mod.updateAvailable') as string)
                          : (t('mod.installed') as string)
                        : undefined
                    }
                    title={mod.repository.metadata.name || modId}
                    description={mod.repository.metadata.description}
                    buttons={[
                      {
                        text: t('mod.details'),
                        onClick: () => replace('/mods-browser/' + modId),
                      },
                    ]}
                    stats={{
                      users: mod.repository.details.users,
                      rating: mod.repository.details.rating,
                    }}
                  />
                ))}
            </ModsGrid>
          </InfiniteScroll>
        </ModsContainer>
      </ContentWrapper>
      {displayedModId && (
        <ContentWrapper>
          <ModDetails
            modId={displayedModId}
            installedModDetails={repositoryMods[displayedModId].installed}
            repositoryModDetails={repositoryMods[displayedModId].repository}
            goBack={() => replace('/mods-browser')}
            installMod={(modSource) =>
              installMod({ modId: displayedModId, modSource })
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
            enableMod={(enable) => enableMod({ modId: displayedModId, enable })}
            editMod={() => editMod({ modId: displayedModId })}
            forkMod={() => forkMod({ modId: displayedModId })}
            deleteMod={() => deleteMod({ modId: displayedModId })}
            updateModRating={(newRating) =>
              updateModRating({ modId: displayedModId, rating: newRating })
            }
          />
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

export default ModsBrowserOnline;
