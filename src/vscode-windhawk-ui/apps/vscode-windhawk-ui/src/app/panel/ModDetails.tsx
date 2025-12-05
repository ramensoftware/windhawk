import { Badge, Button, Card, Radio, Result, Spin, Tooltip } from 'antd';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import {
  useGetModSourceData,
  useGetRepositoryModSourceData,
} from '../webviewIPC';
import { ModConfig, ModMetadata, RepositoryDetails } from '../webviewIPCMessages';
import ModDetailsAdvanced from './ModDetailsAdvanced';
import ModDetailsChangelog from './ModDetailsChangelog';
import ModDetailsHeader, { ModStatus } from './ModDetailsHeader';
import ModDetailsReadme from './ModDetailsReadme';
import ModDetailsSettings from './ModDetailsSettings';
import ModDetailsSource from './ModDetailsSource';
import ModDetailsSourceDiff from './ModDetailsSourceDiff';
import { VersionSelectorModal } from './VersionSelectorModal';
import { mockInstalledModSourceData, mockModVersionSource } from './mockData';

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
  margin-inline-start: auto;
  margin-inline-end: auto;
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
  details?: RepositoryDetails;
};

type ModSourceData = {
  source: string | null;
  metadata: ModMetadata | null;
  readme: string | null;
  initialSettings: any;
};

type TabKey = 'details' | 'settings' | 'code' | 'changelog' | 'advanced' | 'changes';
type ViewMode = 'installed' | 'repository' | 'custom';

interface ModVersionSelectorProps {
  // Version state
  currentView: ViewMode;
  selectedCustomVersion: string | null;

  // Version info (null if not available)
  installed: { version?: string } | null;
  repository: (
    { status: 'loading' } |
    { status: 'loaded'; version?: string } |
    { status: 'failed' } |
    null
  );

  // Callbacks
  onViewChange: (value: Exclude<ViewMode, 'custom'>) => void;
  onOpenVersionModal: () => void;
}

function ModVersionSelector(props: ModVersionSelectorProps) {
  const { t } = useTranslation();
  const {
    currentView,
    selectedCustomVersion,
    installed,
    repository,
    onViewChange,
    onOpenVersionModal,
  } = props;

  if (!installed && !selectedCustomVersion) {
    return null;
  }

  if (!repository) {
    return null;
  }

  return (
    <ModVersionRadioGroup
      size="small"
      value={currentView}
      onChange={(e) => {
        // Don't allow switching to 'custom' value, it will be set after
        // selecting a version in the modal.
        if (e.target.value !== 'custom') {
          onViewChange(e.target.value);
        }
      }}
    >
      {installed && (
        <Radio.Button value="installed">
          {t('modDetails.header.installedVersion')}
          {installed.version && `: ${installed.version}`}
        </Radio.Button>
      )}
      <Radio.Button
        value="repository"
        disabled={repository.status === 'failed'}
      >
        {t('modDetails.header.latestVersion')}
        {repository.status === 'loading'
          ? ': ' + t('modDetails.header.loading')
          : repository.status === 'failed'
            ? ': ' + t('modDetails.header.loadingFailed')
            : repository.status === 'loaded' && repository.version
              ? `: ${repository.version}`
              : ''}
      </Radio.Button>
      <Radio.Button value="custom" onClick={onOpenVersionModal}>
        {selectedCustomVersion
          ? t('modDetails.header.selectedVersion', { version: selectedCustomVersion })
          : t('modDetails.header.otherVersions')}
      </Radio.Button>
    </ModVersionRadioGroup>
  );
}

interface ModDetailsTabContentProps {
  // Tab state
  modId: string;
  currentView: ViewMode;
  activeTab: TabKey;

  // Source data
  modSourceData: ModSourceData | null;

  // Additional source data for changes tab
  installedModSourceData: ModSourceData | null;
  selectedModSourceData: ModSourceData | null;
  installedVersionIsLatest: boolean;

  // Settings tab navigation
  canNavigateAwayRef: React.MutableRefObject<(() => Promise<boolean>) | null>;

  // Retry handler
  onRetryLoad: () => void;
}

function ModDetailsTabContent(props: ModDetailsTabContentProps) {
  const { t } = useTranslation();
  const {
    modId,
    currentView,
    activeTab,
    modSourceData,
    installedModSourceData,
    selectedModSourceData,
    installedVersionIsLatest,
    canNavigateAwayRef,
    onRetryLoad,
  } = props;

  const isLoading = (
    !modSourceData ||
    (activeTab === 'changes' && (
      !installedModSourceData ||
      !selectedModSourceData
    ))
  );
  if (isLoading) {
    const shouldShowLoading = (
      currentView === 'repository' ||
      currentView === 'custom' ||
      activeTab === 'changes');
    if (shouldShowLoading) {
      return <ProgressSpin size="large" tip={t('general.loading')} />;
    }
    return null;
  }

  const isLoadingFailed = (
    (
      currentView === 'repository' ||
      currentView === 'custom' ||
      activeTab === 'changes'
    ) && !selectedModSourceData?.source
  );
  if (isLoadingFailed) {
    return (
      <Result
        status="error"
        title={t('general.loadingFailedTitle')}
        subTitle={t('general.loadingFailedSubtitle')}
        extra={[
          <Button
            type="primary"
            key="try-again"
            onClick={onRetryLoad}
          >
            {t('general.tryAgain')}
          </Button>,
        ]}
      />
    );
  }

  if (activeTab === 'details') {
    return modSourceData.readme ? (
      <ModDetailsReadme markdown={modSourceData.readme} />
    ) : (
      <NoDataMessage>{t('modDetails.details.noData')}</NoDataMessage>
    );
  }

  if (activeTab === 'settings') {
    return modSourceData.initialSettings ? (
      <ModDetailsSettings
        modId={modId}
        initialSettings={modSourceData.initialSettings}
        onCanNavigateAwayChange={(callback) => {
          canNavigateAwayRef.current = callback;
        }}
      />
    ) : (
      <NoDataMessage>{t('modDetails.settings.noData')}</NoDataMessage>
    );
  }

  if (activeTab === 'code') {
    return modSourceData.source ? (
      <ModDetailsSource source={modSourceData.source} />
    ) : (
      <NoDataMessage>{t('modDetails.code.noData')}</NoDataMessage>
    );
  }

  if (activeTab === 'changelog') {
    return (
      <ModDetailsChangelog
        loadingNode={
          <ProgressSpin size="large" tip={t('general.loading')} />
        }
        modId={modId}
      />
    );
  }

  if (activeTab === 'advanced') {
    return <ModDetailsAdvanced modId={modId} />;
  }

  if (activeTab === 'changes') {
    const installedModSource = installedModSourceData?.source ?? null;
    const selectedModSource = selectedModSourceData?.source ?? null;
    if (installedModSource && selectedModSource) {
      return installedVersionIsLatest ? (
        <NoDataMessage>{t('modDetails.changes.noData')}</NoDataMessage>
      ) : (
        <ModDetailsSourceDiff
          oldSource={installedModSource}
          newSource={selectedModSource}
        />
      );
    }
    return <NoDataMessage>{t('modDetails.code.noData')}</NoDataMessage>;
  }

  return null;
}

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

  const [selectedCustomVersion, setSelectedCustomVersion] = useState<string | null>(null);
  const [versionTimestamps, setVersionTimestamps] = useState<Record<string, number>>({});
  const [customVersionSourceData, setCustomVersionSourceData] = useState<ModSourceData | null>(null);
  const [isVersionModalOpen, setIsVersionModalOpen] = useState(false);

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
    setInstalledModSourceData(mockInstalledModSourceData);
    if (installedModDetails?.metadata) {
      getModSourceData({ modId });
    }
  }, [modId, installedModDetails?.metadata, getModSourceData]);

  const { getRepositoryModSourceData } = useGetRepositoryModSourceData(
    useCallback(
      (data) => {
        if (data.modId === modId &&
          (data.version ?? null) === selectedCustomVersion) {
          if (data.version) {
            setCustomVersionSourceData(data.data);
          } else {
            setRepositoryModSourceData(data.data);
          }
        }
      },
      [modId, selectedCustomVersion]
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
    Exclude<ViewMode, 'custom'> | null
  >(null);

  useEffect(() => {
    if (
      !(installedModDetails && (repositoryModDetails || loadRepositoryData))
    ) {
      // Only one type can be selected, reset selection.
      setSelectedModDetails(null);
    }
  }, [installedModDetails, repositoryModDetails, loadRepositoryData]);

  const modDetailsToShow: ViewMode = selectedCustomVersion
    ? 'custom'
    : selectedModDetails || (installedModDetails ? 'installed' : 'repository');

  const [activeTab, setActiveTab] = useState<TabKey>('details');

  // Track if settings can navigate away
  const canNavigateAwayRef = useRef<(() => Promise<boolean>) | null>(null);

  const handleTabChange = useCallback(async (key: string) => {
    // Check if we can navigate away from settings
    if (canNavigateAwayRef.current) {
      const canNavigate = await canNavigateAwayRef.current();
      if (!canNavigate) {
        return;
      }
    }
    setActiveTab(key as TabKey);
  }, []);

  const handleOpenVersionModal = useCallback(() => {
    setIsVersionModalOpen(true);
  }, []);

  const handleVersionSelect = useCallback((version: string, timestamps: Record<string, number>) => {
    setSelectedCustomVersion(version);
    setVersionTimestamps(timestamps);
    setIsVersionModalOpen(false);
    // Fetch the source for the selected version.
    if (mockModVersionSource) {
      setCustomVersionSourceData(mockModVersionSource(version));
    } else {
      setCustomVersionSourceData(null);
      getRepositoryModSourceData({ modId, version });
    }
  }, [getRepositoryModSourceData, modId]);

  const handleClearCustomVersion = useCallback(() => {
    setSelectedCustomVersion(null);
    setVersionTimestamps({});
    setCustomVersionSourceData(null);
  }, []);

  const tabList: Array<{ key: TabKey; tab: React.ReactNode }> = [
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
    const hasLogging = installedModDetails?.config?.loggingEnabled || installedModDetails?.config?.debugLoggingEnabled;
    tabList.push({
      key: 'advanced',
      tab: hasLogging ? (
        <>
          {t('modDetails.advanced.title')}
          {' '}
          <Tooltip title={t('general.loggingEnabled')} placement="bottom">
            <Badge dot status="warning" />
          </Tooltip>
        </>
      ) : t('modDetails.advanced.title'),
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

  // Clear the navigation callback when not on settings tab
  useEffect(() => {
    if (availableActiveTab !== 'settings') {
      canNavigateAwayRef.current = null;
    }
  }, [availableActiveTab]);

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

  if (modDetailsToShow === 'custom') {
    modMetadata = customVersionSourceData?.metadata || {};
    modSourceData = customVersionSourceData;
  } else if (modDetailsToShow === 'installed') {
    modMetadata = installedModMetadata;
    modSourceData = installedModSourceData;
  } else if (modDetailsToShow === 'repository') {
    modMetadata = repositoryModMetadata;
    modSourceData = repositoryModSourceData;
  }

  const installedModSource = installedModSourceData?.source ?? null;
  const repositoryModSource = repositoryModSourceData?.source ?? null;
  const selectedModSourceData = modDetailsToShow === 'custom'
    ? customVersionSourceData
    : repositoryModSourceData;
  const selectedModSource = selectedModSourceData?.source ?? null;

  const installedVersionIsLatest = useMemo(() => {
    return !!(
      selectedModSource &&
      installedModSource &&
      selectedModSource === installedModSource
    );
  }, [selectedModSource, installedModSource]);

  // Determine if the selected custom version is a downgrade
  const isDowngrade = useMemo(() => {
    if (!selectedCustomVersion || !installedModMetadata.version) {
      return false;
    }

    const selectedTimestamp = versionTimestamps[selectedCustomVersion];
    const currentTimestamp = versionTimestamps[installedModMetadata.version];

    return selectedTimestamp !== undefined &&
      currentTimestamp !== undefined &&
      selectedTimestamp < currentTimestamp;
  }, [selectedCustomVersion, installedModMetadata.version, versionTimestamps]);

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
              <ModVersionSelector
                currentView={modDetailsToShow}
                selectedCustomVersion={selectedCustomVersion}
                installed={
                  installedModDetails
                    ? { version: installedModMetadata.version }
                    : null
                }
                repository={
                  !(repositoryModDetails || loadRepositoryData)
                    ? null
                    : !repositoryModDetails && !repositoryModSourceData
                      ? { status: 'loading' }
                      : !repositoryModDetails && !repositoryModSource
                        ? { status: 'failed' }
                        : { status: 'loaded', version: repositoryModMetadata.version }
                }
                onViewChange={(value) => {
                  setSelectedModDetails(value);
                  // Clear custom version when switching back to
                  // installed/latest.
                  handleClearCustomVersion();
                }}
                onOpenVersionModal={handleOpenVersionModal}
              />
            }
            modId={modId}
            modMetadata={modMetadata}
            modConfig={
              (modDetailsToShow === 'installed'
                && installedModDetails?.config)
              || undefined}
            modStatus={modStatus}
            updateAvailable={
              !!(
                installedModDetails &&
                (repositoryModDetails || loadRepositoryData)
              )
            }
            installedVersionIsLatest={installedVersionIsLatest}
            isDowngrade={isDowngrade}
            userRating={installedModDetails?.userRating}
            repositoryDetails={
              (modDetailsToShow === 'repository'
                && repositoryModDetails?.details)
              || undefined}
            callbacks={{
              goBack: props.goBack,
              installMod:
                props.installMod && selectedModSource
                  ? () => props.installMod?.(selectedModSource)
                  : undefined,
              updateMod:
                props.updateMod && selectedModSource
                  ? () => props.updateMod?.(
                    selectedModSource,
                    modStatus === 'disabled'
                  )
                  : undefined,
              forkModFromSource:
                props.forkModFromSource && selectedModSource
                  ? () => props.forkModFromSource?.(selectedModSource)
                  : undefined,
              compileMod: props.compileMod,
              enableMod: props.enableMod,
              editMod: props.editMod,
              forkMod: props.forkMod,
              deleteMod: props.deleteMod,
              updateModRating: props.updateModRating,
              onOpenVersionModal: handleOpenVersionModal,
            }}
          />
        }
        tabList={tabList}
        activeTabKey={availableActiveTab}
        onTabChange={handleTabChange}
      >
        <ModDetailsTabContent
          modId={modId}
          currentView={modDetailsToShow}
          activeTab={availableActiveTab}
          modSourceData={modSourceData}
          installedModSourceData={installedModSourceData}
          selectedModSourceData={selectedModSourceData}
          installedVersionIsLatest={installedVersionIsLatest}
          canNavigateAwayRef={canNavigateAwayRef}
          onRetryLoad={() => {
            if (selectedCustomVersion) {
              getRepositoryModSourceData({
                modId,
                version: selectedCustomVersion,
              });
            } else if (repositoryModDetails || loadRepositoryData) {
              getRepositoryModSourceData({ modId });
            }
          }}
        />
      </ModDetailsCard>
      <VersionSelectorModal
        modId={modId}
        open={isVersionModalOpen}
        selectedVersion={selectedCustomVersion}
        onSelect={handleVersionSelect}
        onCancel={() => setIsVersionModalOpen(false)}
      />
    </ModDetailsContainer>
  );
}

export default ModDetails;
