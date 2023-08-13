import { Button, Card, Radio, Result, Spin } from 'antd';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import {
  useGetModSourceData,
  useGetRepositoryModSourceData,
} from '../webviewIPC';
import { ModConfig, ModMetadata } from '../webviewIPCMessages';
import ModDetailsAdvanced from './ModDetailsAdvanced';
import ModDetailsChangelog from './ModDetailsChangelog';
import ModDetailsHeader, { ModStatus } from './ModDetailsHeader';
import ModDetailsReadme from './ModDetailsReadme';
import ModDetailsSettings from './ModDetailsSettings';
import ModDetailsSource from './ModDetailsSource';
import ModDetailsSourceDiff from './ModDetailsSourceDiff';

const ModDetailsContainer = styled.div`
  flex: 1;
  padding-top: 20px;
`;

const ModDetailsCard = styled(Card)`
  min-height: 100%;
  border-bottom: none;
  border-bottom-left-radius: 0;
  border-bottom-right-radius: 0;
`;

const ModVersionRadioGroup = styled(Radio.Group)`
  font-weight: normal;
  margin-bottom: 8px;
`;

const ProgressSpin = styled(Spin)`
  display: block;
  margin-left: auto;
  margin-right: auto;
  font-size: 32px;
`;

const NoDataMessage = styled.div`
  color: rgba(255, 255, 255, 0.45);
  font-style: italic;
`;

type InstalledModDetails = {
  metadata: ModMetadata | null;
  config: ModConfig | null;
  userRating?: number;
};

type RepositoryModDetails = {
  metadata?: ModMetadata;
};

type ModSourceData = {
  source: string | null;
  metadata: ModMetadata | null;
  readme: string | null;
  initialSettings: any;
};

interface Props {
  modId: string;
  installedModDetails?: InstalledModDetails;
  repositoryModDetails?: RepositoryModDetails;
  loadRepositoryData?: boolean;
  goBack: () => void;
  installMod?: (modSource: string) => void;
  updateMod?: (modSource: string, disabled: boolean) => void;
  forkModFromSource?: (modSource: string) => void;
  compileMod: () => void;
  enableMod: (enable: boolean) => void;
  editMod: () => void;
  forkMod: () => void;
  deleteMod: () => void;
  updateModRating: (newRating: number) => void;
}

function ModDetails(props: Props) {
  const { t } = useTranslation();

  const {
    modId,
    installedModDetails,
    repositoryModDetails,
    loadRepositoryData,
  } = props;

  const isLocalMod = modId.startsWith('local@');

  const [installedModSourceData, setInstalledModSourceData] =
    useState<ModSourceData | null>(null);
  const [repositoryModSourceData, setRepositoryModSourceData] =
    useState<ModSourceData | null>(null);

  const { getModSourceData } = useGetModSourceData(
    useCallback(
      (data) => {
        if (data.modId === modId) {
          setInstalledModSourceData(data.data);
        }
      },
      [modId]
    )
  );

  useEffect(() => {
    setInstalledModSourceData(null);
    if (installedModDetails?.metadata) {
      getModSourceData({ modId });
    }
  }, [modId, installedModDetails?.metadata, getModSourceData]);

  const { getRepositoryModSourceData } = useGetRepositoryModSourceData(
    useCallback(
      (data) => {
        if (data.modId === modId) {
          setRepositoryModSourceData(data.data);
        }
      },
      [modId]
    )
  );

  useEffect(() => {
    setRepositoryModSourceData(null);
    if (repositoryModDetails || loadRepositoryData) {
      getRepositoryModSourceData({ modId });
    }
  }, [
    getRepositoryModSourceData,
    loadRepositoryData,
    modId,
    repositoryModDetails,
  ]);

  const [selectedModDetails, setSelectedModDetails] = useState<
    'installed' | 'repository' | null
  >(null);

  useEffect(() => {
    if (
      !(installedModDetails && (repositoryModDetails || loadRepositoryData))
    ) {
      // Only one type can be selected, reset selection.
      setSelectedModDetails(null);
    }
  }, [installedModDetails, repositoryModDetails, loadRepositoryData]);

  const modDetailsToShow =
    selectedModDetails || (installedModDetails ? 'installed' : 'repository');

  const [activeTab, setActiveTab] = useState('details');

  const tabList = [
    {
      key: 'details',
      tab: t('modDetails.details.title'),
    },
  ];

  if (modDetailsToShow === 'installed' && installedModDetails?.config) {
    tabList.push({
      key: 'settings',
      tab: t('modDetails.settings.title'),
    });
  }

  tabList.push({
    key: 'code',
    tab: t('modDetails.code.title'),
  });

  if (!isLocalMod) {
    tabList.push({
      key: 'changelog',
      tab: t('modDetails.changelog.title'),
    });
  }

  if (modDetailsToShow === 'installed') {
    tabList.push({
      key: 'advanced',
      tab: t('modDetails.advanced.title'),
    });
  }

  if (installedModDetails && (repositoryModDetails || loadRepositoryData)) {
    tabList.push({
      key: 'changes',
      tab: t('modDetails.changes.title'),
    });
  }

  const availableActiveTab = tabList.find((x) => x.key === activeTab)
    ? activeTab
    : 'details';

  let installedModMetadata: ModMetadata = {};
  if (installedModSourceData?.metadata) {
    installedModMetadata = installedModSourceData.metadata;
  } else if (installedModDetails) {
    installedModMetadata = installedModDetails.metadata || {};
  }

  let repositoryModMetadata: ModMetadata = {};
  if (repositoryModSourceData?.metadata) {
    repositoryModMetadata = repositoryModSourceData.metadata;
  } else if (repositoryModDetails?.metadata) {
    repositoryModMetadata = repositoryModDetails.metadata;
  }

  let modMetadata: ModMetadata = {};
  let modSourceData: ModSourceData | null = null;

  if (modDetailsToShow === 'installed') {
    modMetadata = installedModMetadata;
    modSourceData = installedModSourceData;
  } else if (modDetailsToShow === 'repository') {
    modMetadata = repositoryModMetadata;
    modSourceData = repositoryModSourceData;
  }

  const installedModSource = installedModSourceData?.source ?? null;
  const repositoryModSource = repositoryModSourceData?.source ?? null;

  const installedVersionIsLatest = useMemo(() => {
    return !!(
      repositoryModSource &&
      installedModSource &&
      repositoryModSource === installedModSource
    );
  }, [repositoryModSource, installedModSource]);

  let modStatus: ModStatus = 'not-installed';
  if (modDetailsToShow === 'installed' && installedModDetails) {
    if (!installedModDetails.config) {
      modStatus = 'installed-not-compiled';
    } else if (!installedModDetails.config.disabled) {
      modStatus = 'enabled';
    } else {
      modStatus = 'disabled';
    }
  }

  return (
    <ModDetailsContainer>
      <ModDetailsCard
        title={
          <ModDetailsHeader
            topNode={
              installedModDetails &&
              (repositoryModDetails || loadRepositoryData) && (
                <ModVersionRadioGroup
                  size="small"
                  value={modDetailsToShow}
                  onChange={(e) => setSelectedModDetails(e.target.value)}
                >
                  <Radio.Button value="installed">
                    {t('modDetails.header.installedVersion')}
                    {installedModMetadata.version &&
                      `: ${installedModMetadata.version}`}
                  </Radio.Button>
                  <Radio.Button
                    value="repository"
                    disabled={!repositoryModDetails && !repositoryModSource}
                  >
                    {t('modDetails.header.latestVersion')}
                    {!repositoryModDetails && !repositoryModSourceData
                      ? ': ' + t('modDetails.header.loading')
                      : !repositoryModDetails && !repositoryModSource
                      ? ': ' + t('modDetails.header.loadingFailed')
                      : repositoryModMetadata.version &&
                        `: ${repositoryModMetadata.version}`}
                  </Radio.Button>
                </ModVersionRadioGroup>
              )
            }
            modId={modId}
            modMetadata={modMetadata}
            modStatus={modStatus}
            updateAvailable={
              !!(
                installedModDetails &&
                (repositoryModDetails || loadRepositoryData)
              )
            }
            installedVersionIsLatest={installedVersionIsLatest}
            userRating={installedModDetails?.userRating}
            callbacks={{
              goBack: props.goBack,
              installMod:
                props.installMod && repositoryModSource
                  ? () =>
                      repositoryModSource &&
                      props.installMod?.(repositoryModSource)
                  : undefined,
              updateMod:
                props.updateMod && repositoryModSource
                  ? () =>
                      repositoryModSource &&
                      props.updateMod?.(
                        repositoryModSource,
                        modStatus === 'disabled'
                      )
                  : undefined,
              forkModFromSource:
                props.forkModFromSource && repositoryModSource
                  ? () =>
                      repositoryModSource &&
                      props.forkModFromSource?.(repositoryModSource)
                  : undefined,
              compileMod: props.compileMod,
              enableMod: props.enableMod,
              editMod: props.editMod,
              forkMod: props.forkMod,
              deleteMod: props.deleteMod,
              updateModRating: props.updateModRating,
            }}
          />
        }
        tabList={tabList}
        activeTabKey={availableActiveTab}
        onTabChange={(key) => setActiveTab(key)}
      >
        {!modSourceData ||
        (availableActiveTab === 'changes' && !repositoryModSourceData) ? (
          modDetailsToShow === 'repository' ||
          availableActiveTab === 'changes' ? (
            <ProgressSpin size="large" tip={t('general.loading')} />
          ) : (
            ''
          )
        ) : (modDetailsToShow === 'repository' ||
            availableActiveTab === 'changes') &&
          !repositoryModSource ? (
          <Result
            status="error"
            title={t('general.loadingFailedTitle')}
            subTitle={t('general.loadingFailedSubtitle')}
            extra={[
              <Button
                type="primary"
                key="try-again"
                onClick={() => {
                  setRepositoryModSourceData(null);
                  if (repositoryModDetails || loadRepositoryData) {
                    getRepositoryModSourceData({ modId });
                  }
                }}
              >
                {t('general.tryAgain')}
              </Button>,
            ]}
          />
        ) : availableActiveTab === 'details' ? (
          modSourceData.readme ? (
            <ModDetailsReadme markdown={modSourceData.readme} />
          ) : (
            <NoDataMessage>{t('modDetails.details.noData')}</NoDataMessage>
          )
        ) : availableActiveTab === 'settings' ? (
          modSourceData.initialSettings ? (
            <ModDetailsSettings
              modId={modId}
              initialSettings={modSourceData.initialSettings}
            />
          ) : (
            <NoDataMessage>{t('modDetails.settings.noData')}</NoDataMessage>
          )
        ) : availableActiveTab === 'code' ? (
          modSourceData.source ? (
            <ModDetailsSource source={modSourceData.source} />
          ) : (
            <NoDataMessage>{t('modDetails.code.noData')}</NoDataMessage>
          )
        ) : availableActiveTab === 'changelog' ? (
          <ModDetailsChangelog
            loadingNode={
              <ProgressSpin size="large" tip={t('general.loading')} />
            }
            modId={modId}
          />
        ) : availableActiveTab === 'advanced' ? (
          <ModDetailsAdvanced modId={modId} />
        ) : availableActiveTab === 'changes' ? (
          installedModSource && repositoryModSource ? (
            installedVersionIsLatest ? (
              <NoDataMessage>{t('modDetails.changes.noData')}</NoDataMessage>
            ) : (
              <ModDetailsSourceDiff
                oldSource={installedModSource}
                newSource={repositoryModSource}
              />
            )
          ) : (
            <NoDataMessage>{t('modDetails.code.noData')}</NoDataMessage>
          )
        ) : (
          '???'
        )}
      </ModDetailsCard>
    </ModDetailsContainer>
  );
}

export default ModDetails;
