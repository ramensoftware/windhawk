import { isLocalModId } from '@app/utils';
import type { InitialSettings, ModConfig, ModMetadata, RepositoryDetails } from '@app/webviewIPCMessages';
import { Badge, Button, Card, Radio, Result, Spin, Tooltip } from 'antd';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled, { css } from 'styled-components';
import ModDetailsHeader, { type ModStatus } from './ModDetailsHeader';
import ModDetailsChangelog from './tabs/ModDetailsChangelog';
import ModDetailsReadme from './tabs/ModDetailsReadme';
import ModDetailsSettings from './tabs/settings';
import ModDetailsSource from './tabs/ModDetailsSource';
/// #if EXTENSION
import ModDetailsAdvanced from './tabs/ModDetailsAdvanced';
import ModDetailsSourceDiff from './tabs/ModDetailsSourceDiff';
import { VersionSelectorModal } from './VersionSelectorModal';
/// #endif

declare const WEBPACK_IS_WEBSITE: boolean;

const ModDetailsContainer = styled.div`
  flex: 1;
  padding-top: 20px;
`;

const ModDetailsCard = styled(Card)`
  min-height: 100%;
  ${!WEBPACK_IS_WEBSITE && css`
    border-bottom: none;
    border-bottom-left-radius: 0;
    border-bottom-right-radius: 0;
  `}
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
  color: var(--whui-text-muted);
  font-style: italic;
`;

export type ModSourceData = {
  source: string | null;
  metadata: ModMetadata | null;
  readme: string | null;
  initialSettings: InitialSettings | null;
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
  isLocalMod: boolean;
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
  onRetryLoad?: () => void;
}

function ModDetailsTabContent(props: ModDetailsTabContentProps) {
  const { t } = useTranslation();
  const {
    modId,
    isLocalMod,
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
      return <ProgressSpin size="large" tip={t('general.status.loading')} />;
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
        title={t('general.status.loadingFailedTitle')}
        subTitle={t('general.status.loadingFailedSubtitle')}
        extra={onRetryLoad ? [
          <Button
            type="primary"
            key="try-again"
            onClick={onRetryLoad}
          >
            {t('general.status.tryAgain')}
          </Button>,
        ] : undefined}
      />
    );
  }

  if (activeTab === 'details') {
    return modSourceData.readme ? (
      <ModDetailsReadme markdown={modSourceData.readme} isLocalMod={isLocalMod} />
    ) : (
      <NoDataMessage>{t('modDetails.details.noData')}</NoDataMessage>
    );
  }

  if (activeTab === 'settings') {
    return modSourceData.initialSettings ? (
      <ModDetailsSettings
        // Remount when toggling between the editable (installed) and read-only
        // views so leftover editor state does not persist. Without this, removing
        // the mod while its settings editor is open leaves the YAML editor visible
        // and keeps prompting to discard unsaved changes when navigating away.
        key={currentView === 'installed' ? 'editable' : 'readonly'}
        modId={modId}
        initialSettings={modSourceData.initialSettings}
        readOnly={currentView !== 'installed'}
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
          <ProgressSpin size="large" tip={t('general.status.loading')} />
        }
        modId={modId}
      />
    );
  }

  /// #if EXTENSION
  if (activeTab === 'advanced') {
    return <ModDetailsAdvanced modId={modId} />;
  }
  /// #endif

  /// #if EXTENSION
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
  /// #endif

  return null;
}

export interface ModDetailsViewProps {
  modId: string;
  goBack: () => void;

  // Mod metadata (can be from repository, installed, or custom version)
  modMetadata: ModMetadata;
  repositoryDetails?: RepositoryDetails;

  // Source data for current view
  modSourceData: ModSourceData | null;

  // For changes tab (extension mode only)
  installedModSourceData?: ModSourceData | null;
  selectedModSourceData?: ModSourceData | null;
  installedVersionIsLatest?: boolean;

  // Extension-specific props (optional, flat structure)
  extensionViewProps?: {
    // Version selector state
    currentView: ViewMode;
    selectedCustomVersion: string | null;
    installedVersion?: string;
    repositoryStatus: (
      { status: 'loading' } |
      { status: 'loaded'; version?: string } |
      { status: 'failed' } |
      null
    );
    onViewChange: (value: Exclude<ViewMode, 'custom'>) => void;
    onVersionSelect: (version: string, timestamps: Record<string, number>) => void;

    // Mod state (used by View for tabs AND passed to Header)
    modConfig?: ModConfig;
    modStatus: ModStatus;
    updateAvailable: boolean;
    isDowngrade: boolean;
    userRating?: number;

    // Action callbacks (passed to Header)
    callbacks: {
      installMod?: () => void;
      updateMod?: () => void;
      forkModFromSource?: () => void;
      compileMod: () => void;
      enableMod: (enable: boolean) => void;
      editMod: () => void;
      forkMod: () => void;
      deleteMod: () => void;
      updateModRating: (newRating: number) => void;
    };
  };

  // Retry handler for failed loads
  onRetryLoad?: () => void;
}

export function ModDetailsView(props: ModDetailsViewProps) {
  const { t } = useTranslation();
  const {
    modId,
    goBack,
    modMetadata,
    repositoryDetails,
    modSourceData,
    installedModSourceData = null,
    selectedModSourceData = null,
    installedVersionIsLatest = false,
    extensionViewProps,
    onRetryLoad,
  } = props;

  const isLocalMod = isLocalModId(modId);

  // Internal UI state
  const [activeTab, setActiveTab] = useState<TabKey>('details');
  const [isVersionModalOpen, setIsVersionModalOpen] = useState(false);
  const canNavigateAwayRef = useRef<(() => Promise<boolean>) | null>(null);

  const handleOpenVersionModal = useCallback(() => {
    setIsVersionModalOpen(true);
  }, []);

  const handleVersionSelect = useCallback((version: string, timestamps: Record<string, number>) => {
    setIsVersionModalOpen(false);
    extensionViewProps?.onVersionSelect?.(version, timestamps);
  }, [extensionViewProps]);

  const handleVersionModalCancel = useCallback(() => {
    setIsVersionModalOpen(false);
  }, []);

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

  // Determine current view mode (flat access)
  const currentView: ViewMode = extensionViewProps?.currentView ?? 'repository';
  const hasInstalledVersion = extensionViewProps?.installedVersion !== undefined;

  // Build tab list dynamically
  const tabHeader = (key: TabKey, label: React.ReactNode) => ({
    key,
    tab: <span data-testid={`mod-details-tab-${key}`}>{label}</span>,
  });

  const tabList: Array<{ key: TabKey; tab: React.ReactNode }> = [
    tabHeader('details', t('modDetails.details.title')),
    tabHeader('settings', t('modDetails.settings.title')),
    tabHeader('code', t('modDetails.code.title')),
  ];

  if (!isLocalMod) {
    tabList.push(tabHeader('changelog', t('modDetails.changelog.title')));
  }

  if (currentView === 'installed' && extensionViewProps) {
    const hasLogging = extensionViewProps.modConfig?.loggingEnabled ||
      extensionViewProps.modConfig?.debugLoggingEnabled;
    tabList.push(
      tabHeader(
        'advanced',
        hasLogging ? (
          <>
            {t('modDetails.advanced.title')}
            {' '}
            <Tooltip title={t('general.status.loggingEnabled')} placement="bottom">
              <Badge dot status="warning" />
            </Tooltip>
          </>
        ) : t('modDetails.advanced.title')
      )
    );
  }

  if (hasInstalledVersion && extensionViewProps?.updateAvailable) {
    tabList.push(tabHeader('changes', t('modDetails.changes.title')));
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

  // Build version selector props from flat extensionViewProps
  const versionSelectorNode = extensionViewProps && (
    <ModVersionSelector
      currentView={extensionViewProps.currentView}
      selectedCustomVersion={extensionViewProps.selectedCustomVersion}
      installed={extensionViewProps.installedVersion !== undefined
        ? { version: extensionViewProps.installedVersion }
        : null}
      repository={extensionViewProps.repositoryStatus}
      onViewChange={extensionViewProps.onViewChange}
      onOpenVersionModal={handleOpenVersionModal}
    />
  );

  // Build header props from flat extensionViewProps
  const extensionHeaderProps = extensionViewProps && {
    modConfig: extensionViewProps.modConfig,
    modStatus: extensionViewProps.modStatus,
    updateAvailable: extensionViewProps.updateAvailable,
    installedVersionIsLatest,
    isDowngrade: extensionViewProps.isDowngrade,
    userRating: extensionViewProps.userRating,
    callbacks: {
      ...extensionViewProps.callbacks,
      onOpenVersionModal: handleOpenVersionModal,
    },
  };

  return (
    <ModDetailsContainer data-testid="mod-details" data-mod-id={modId}>
      <ModDetailsCard
        title={
          <ModDetailsHeader
            topNode={versionSelectorNode}
            modId={modId}
            modMetadata={modMetadata}
            repositoryDetails={repositoryDetails}
            goBack={goBack}
            extensionHeaderProps={extensionHeaderProps}
          />
        }
        tabList={tabList}
        activeTabKey={availableActiveTab}
        onTabChange={handleTabChange}
        tabProps={{
          // Show "..." button in mobile view as well.
          // https://github.com/ant-design/ant-design/issues/27341#issuecomment-1043129599
          renderTabBar: (props, TabNavList) => (
            <TabNavList {...props} mobile={false} />
          ),
        }}
      >
        <ModDetailsTabContent
          modId={modId}
          isLocalMod={isLocalMod}
          currentView={currentView}
          activeTab={availableActiveTab}
          modSourceData={modSourceData}
          installedModSourceData={installedModSourceData}
          selectedModSourceData={selectedModSourceData}
          installedVersionIsLatest={installedVersionIsLatest}
          canNavigateAwayRef={canNavigateAwayRef}
          onRetryLoad={onRetryLoad}
        />
      </ModDetailsCard>
      {
        /// #if EXTENSION
        extensionViewProps?.onVersionSelect && (
          <VersionSelectorModal
            modId={modId}
            open={isVersionModalOpen}
            selectedVersion={extensionViewProps.selectedCustomVersion}
            onSelect={handleVersionSelect}
            onCancel={handleVersionModalCancel}
          />
        )
        /// #endif
      }
    </ModDetailsContainer >
  );
}
