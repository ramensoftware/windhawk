import { faHdd, faStar } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Button, Empty, Modal, Spin } from 'antd';
import { produce } from 'immer';
import { useCallback, useContext, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';
import styled, { css } from 'styled-components';
import { AppUISettingsContext } from '../appUISettings';
import {
  editMod,
  forkMod,
  useCompileMod,
  useDeleteMod,
  useEnableMod,
  useGetFeaturedMods,
  useGetInstalledMods,
  useInstallMod,
  useUpdateInstalledModsDetails,
  useUpdateModRating,
} from '../webviewIPC';
import {
  ModConfig,
  ModMetadata,
  RepositoryDetails,
} from '../webviewIPCMessages';
import {
  mockModsBrowserLocalFeaturedMods,
  mockModsBrowserLocalInitialMods,
} from './mockData';
import ModCard from './ModCard';
import ModDetails from './ModDetails';

const SectionText = styled.h2`
  margin-top: 20px;
`;

const SectionIcon = styled(FontAwesomeIcon)`
  margin-right: 3px;
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

const ExploreModsButton = styled(Button)`
  height: 100%;
  font-size: 22px;
`;

const ProgressSpin = styled(Spin)`
  display: block;
  margin-left: auto;
  margin-right: auto;
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
  const replace = useCallback(
    (to: string) => navigate(to, { replace: true }),
    [navigate]
  );

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

  const installedModsSorted = useMemo(() => {
    if (!installedMods) {
      return installedMods;
    }

    return Object.entries(installedMods).sort((a, b) => {
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
  }, [installedMods]);

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
            replace('/');
          }

          setInstalledMods(
            produce(installedMods, (draft) => {
              delete draft[modId];
            })
          );
        }
      },
      [displayedModId, displayedModType, installedMods, replace]
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

  if (!installedMods || !installedModsSorted) {
    return null;
  }

  const noInstalledMods = installedModsSorted.length === 0;

  return (
    <>
      <ContentWrapper $hidden={!!displayedModId}>
        <ModsContainer $extraBottomPadding={!devModeOptOut}>
          <SectionText>
            <SectionIcon icon={faHdd} /> {t('home.installedMods.title')}
          </SectionText>
          {noInstalledMods ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t('home.installedMods.noMods')}
            >
              <Button type="primary" onClick={() => replace('/mods-browser')}>
                {t('home.browse')}
              </Button>
            </Empty>
          ) : (
            <ModsGrid>
              {installedModsSorted.map(([modId, mod]) => (
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
                      onClick: () => replace('/mods/local/' + modId),
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
          )}
          <SectionText>
            <SectionIcon icon={faStar} /> {t('home.featuredMods.title')}
          </SectionText>
          {featuredModsFilteredAndSorted === undefined ? (
            <ProgressSpin size="large" tip={t('general.loading')} />
          ) : featuredModsFilteredAndSorted === null ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t('general.loadingFailed')}
            >
              <Button type="primary" onClick={() => replace('/mods-browser')}>
                {t('home.browse')}
              </Button>
            </Empty>
          ) : featuredModsFilteredAndSorted.length === 0 ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t('home.featuredMods.noMods')}
            >
              <Button type="primary" onClick={() => replace('/mods-browser')}>
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
                  buttons={[
                    {
                      text: t('mod.details'),
                      onClick: () => replace('/mods/featured/' + modId),
                    },
                  ]}
                  stats={{
                    users: mod.details.users,
                    rating: mod.details.rating,
                  }}
                />
              ))}
              <ExploreModsButton
                size="large"
                onClick={() => replace('/mods-browser')}
              >
                {t('home.featuredMods.explore')}
              </ExploreModsButton>
            </ModsGrid>
          )}
        </ModsContainer>
      </ContentWrapper>
      {displayedModId && (
        <ContentWrapper>
          {displayedModType === 'local' ? (
            <ModDetails
              modId={displayedModId}
              installedModDetails={installedMods[displayedModId]}
              loadRepositoryData={installedMods[displayedModId].updateAvailable}
              goBack={() => replace('/')}
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
          ) : featuredMods ? (
            <ModDetails
              modId={displayedModId}
              installedModDetails={installedMods[displayedModId]}
              repositoryModDetails={featuredMods[displayedModId]}
              goBack={() => replace('/')}
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
          ) : (
            ''
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
