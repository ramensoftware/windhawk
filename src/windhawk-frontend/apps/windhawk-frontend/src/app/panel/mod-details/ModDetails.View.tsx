import { isLocalModId, readStoredValue, writeStoredValue } from '@app/utils';
import type { GetModSourceDataReplyData, ModMetadata, RepositoryDetails } from '@app/webviewIPCMessages';
import { Badge, Button, Card, Radio, Result, Spin, Tooltip } from 'antd';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled, { css } from 'styled-components';
import { compareToRepository } from '../shared/updateOffer';
import ModDetailsHeader, { type ExtensionHeaderProps } from './ModDetailsHeader';
import type { ModDetailsState, ShownVersion } from './modDetailsState';
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

// A source read as the host answers one, taken from the reply type rather than
// restated. The repository's reply carries the same shape, and a read that
// failed is this with every field null rather than nothing at all - which is
// what tells it from a read still on its way.
export type ModSourceData = GetModSourceDataReplyData['data'];

const TAB_KEYS = [
  'details',
  'settings',
  'code',
  'changelog',
  'advanced',
  'changes',
] as const;

type TabKey = (typeof TAB_KEYS)[number];

// Where the tab a screen was left on is kept for the next time it is drawn from
// scratch.
const ACTIVE_TAB_STORAGE_KEY = 'modDetailsActiveTab';

// Anything stored that no longer names a tab reads as nothing stored at all, so
// a key written by a build that had a tab this one does not opens on the first.
function readStoredTab(): TabKey | null {
  const stored = readStoredValue(ACTIVE_TAB_STORAGE_KEY);
  return TAB_KEYS.find((key) => key === stored) ?? null;
}

// Where the repository side of a mod stands. Two facts, held apart because they
// answer apart: a version can be named by a listing or by the host's cache
// without the source having been read, and a read can fail over a version that
// is perfectly well known.
export type RepositoryStatus = {
  // Whether the repository's source for this mod has arrived.
  read: 'loading' | 'loaded' | 'failed';
  // The version the repository holds, absent while no side has named one.
  version?: string;
};

// The values the radio group carries, which name the same three views the state
// does. They are what the DOM addresses these buttons by.
const VIEW_VALUES = {
  installed: 'installed',
  latest: 'repository',
  picked: 'custom',
} as const;

interface ModVersionSelectorProps {
  // Version state
  isLocalMod: boolean;
  state: ModDetailsState;
  repository: RepositoryStatus | null;

  // Callbacks
  onShowVersion: (kind: 'installed' | 'latest') => void;
  onOpenVersionModal: () => void;
}

function ModVersionSelector(props: ModVersionSelectorProps) {
  const { t } = useTranslation();
  const { isLocalMod, state, repository, onShowVersion, onOpenVersionModal } = props;
  const { installed, shown } = state;

  // The versions are the repository's, and a local mod has none: it is the one
  // copy of itself, with nothing to switch between.
  if (isLocalMod) {
    return null;
  }

  // The latest version is a way in only when it is not the installed one - that
  // button already shows it. The view is never left on it in that state either:
  // two views naming one version are one view, under the name further in. A side
  // that has named no version is a way in, being nothing to tell it is the same
  // version.
  const showLatestVersion =
    !!repository &&
    compareToRepository(installed?.metadata?.version, repository.version)
      .kind !== 'upToDate';

  // What the latest version reads as: the read that failed, whichever version is
  // known, or the wait for one. Naming the failure over a version that is known
  // is the point - the version is not what failed to arrive, the source behind
  // it is, and it is that source the button leads to.
  const latestVersionLabel =
    repository &&
    (repository.read === 'failed'
      ? t('modDetails.header.loadingFailed')
      : (repository.version ??
        (repository.read === 'loading'
          ? t('modDetails.header.loading')
          : null)));

  return (
    <ModVersionRadioGroup
      size="small"
      value={VIEW_VALUES[shown.kind]}
      onChange={(e) => {
        // Don't allow switching to the picked version's value, it will be set
        // after selecting a version in the modal.
        if (e.target.value === VIEW_VALUES.installed) {
          onShowVersion('installed');
        } else if (e.target.value === VIEW_VALUES.latest) {
          onShowVersion('latest');
        }
      }}
    >
      {installed && (
        <Radio.Button value={VIEW_VALUES.installed}>
          {t('modDetails.header.installedVersion')}
          {installed.metadata?.version && `: ${installed.metadata.version}`}
        </Radio.Button>
      )}
      {/* Enabled through a failed read: the view behind it is where the retry
          is, and a button that cannot be pressed leaves a reader on the
          installed version with no way to ask for the other one again. */}
      {showLatestVersion && (
        <Radio.Button value={VIEW_VALUES.latest}>
          {t('modDetails.header.latestVersion')}
          {latestVersionLabel && `: ${latestVersionLabel}`}
        </Radio.Button>
      )}
      <Radio.Button value={VIEW_VALUES.picked} onClick={onOpenVersionModal}>
        {shown.kind === 'picked'
          ? t('modDetails.header.selectedVersion', { version: shown.version })
          : t('modDetails.header.otherVersions')}
      </Radio.Button>
    </ModVersionRadioGroup>
  );
}

interface ModDetailsTabContentProps {
  // Tab state
  modId: string;
  isLocalMod: boolean;
  shown: ShownVersion;
  activeTab: TabKey;

  // Source data
  modSourceData: ModSourceData | null;

  // Additional source data for changes tab
  installedModSourceData: ModSourceData | null;
  selectedModSourceData: ModSourceData | null;

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
    shown,
    activeTab,
    modSourceData,
    installedModSourceData,
    selectedModSourceData,
    canNavigateAwayRef,
    onRetryLoad,
  } = props;

  // The sources the tab on screen reads. The changelog fetches its own by mod
  // id and the advanced tab takes nothing else, so neither waits on a source nor
  // fails with one; the tab that diffs reads both sides of the diff, whichever
  // version the screen happens to be showing.
  const sourcesRead: (ModSourceData | null)[] =
    activeTab === 'changelog' || activeTab === 'advanced'
      ? []
      : activeTab === 'changes'
        ? [installedModSourceData, selectedModSourceData]
        : [modSourceData];

  // The installed source is read off the machine and lands in a frame, so the
  // wait for it draws nothing rather than flashing a spinner over it. A version
  // fetched from the repository is worth saying the screen is waiting for.
  const showsSpinnerWhileLoading =
    shown.kind !== 'installed' || activeTab === 'changes';

  if (sourcesRead.some((sourceData) => !sourceData)) {
    if (showsSpinnerWhileLoading) {
      return <ProgressSpin size="large" tip={t('general.status.loading')} />;
    }
    return null;
  }

  // A source that arrived carrying none is a read that failed, whichever version
  // it was for: the copy on the machine goes unreadable like any other, and the
  // tab that diffs it against the repository fails on either side.
  if (sourcesRead.some((sourceData) => !sourceData?.source)) {
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
    return modSourceData?.readme ? (
      <ModDetailsReadme markdown={modSourceData.readme} isLocalMod={isLocalMod} />
    ) : (
      <NoDataMessage>{t('modDetails.details.noData')}</NoDataMessage>
    );
  }

  if (activeTab === 'settings') {
    return modSourceData?.initialSettings ? (
      <ModDetailsSettings
        // Remount when toggling between the editable (installed) and read-only
        // views so leftover editor state does not persist. Without this, removing
        // the mod while its settings editor is open leaves the YAML editor visible
        // and keeps prompting to discard unsaved changes when navigating away.
        key={shown.kind === 'installed' ? 'editable' : 'readonly'}
        modId={modId}
        initialSettings={modSourceData.initialSettings}
        readOnly={shown.kind !== 'installed'}
        onCanNavigateAwayChange={(callback) => {
          canNavigateAwayRef.current = callback;
        }}
      />
    ) : (
      <NoDataMessage>{t('modDetails.settings.noData')}</NoDataMessage>
    );
  }

  if (activeTab === 'code') {
    return modSourceData?.source ? (
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
    // Both sides are here: the checks above waited for each and reported the one
    // that failed.
    const installedModSource = installedModSourceData?.source ?? '';
    const selectedModSource = selectedModSourceData?.source ?? '';
    // The version on screen being the source already installed is what leaves
    // the tab with nothing to show.
    return installedModSource === selectedModSource ? (
      <NoDataMessage>{t('modDetails.changes.noData')}</NoDataMessage>
    ) : (
      <ModDetailsSourceDiff
        oldSource={installedModSource}
        newSource={selectedModSource}
      />
    );
  }
  /// #endif

  return null;
}

export interface ModDetailsViewProps {
  modId: string;
  goBack?: () => void;

  // Mod metadata (can be from repository, installed, or custom version)
  modMetadata: ModMetadata;
  repositoryDetails?: RepositoryDetails;

  // Source data for current view
  modSourceData: ModSourceData | null;

  // For changes tab (extension mode only)
  installedModSourceData?: ModSourceData | null;
  selectedModSourceData?: ModSourceData | null;

  // Extension-specific props: what the header renders, plus what the tabs and
  // the version selector need on top of it.
  extensionViewProps?: ExtensionHeaderProps & {
    // Whether the tab on screen outlives the screen itself, for an owner whose
    // screen is drawn from scratch again on its own rather than at a reader's
    // request. Where it is not, opening the mod starts at its details.
    remembersActiveTab: boolean;

    // The mod as it sits on the machine, and which of its versions is on screen.
    state: ModDetailsState;

    // Version selector state
    repositoryStatus: RepositoryStatus | null;
    onShowVersion: (kind: 'installed' | 'latest') => void;
    onVersionSelect: (version: string, timestamps: Record<string, number>) => void;
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
    extensionViewProps,
    onRetryLoad,
  } = props;

  const isLocalMod = isLocalModId(modId);

  // Internal UI state
  const remembersActiveTab = !!extensionViewProps?.remembersActiveTab;
  // The tab a mod opens on: the one this screen was left on where that outlives
  // the screen, and the mod's details otherwise.
  const openingTab = () =>
    (remembersActiveTab ? readStoredTab() : null) ?? 'details';
  const [activeTab, setActiveTab] = useState<TabKey>(openingTab);

  // The mod on screen can change without this component being built again, and
  // the tab a reader is on is one they chose for the mod they chose it under.
  const [shownModId, setShownModId] = useState(modId);
  if (shownModId !== modId) {
    setShownModId(modId);
    setActiveTab(openingTab());
  }

  const [isVersionModalOpen, setIsVersionModalOpen] = useState(false);
  const canNavigateAwayRef = useRef<(() => Promise<boolean>) | null>(null);

  const handleOpenVersionModal = useCallback(() => {
    setIsVersionModalOpen(true);
  }, []);

  const handleVersionSelect = useCallback((version: string, timestamps: Record<string, number>) => {
    setIsVersionModalOpen(false);
    extensionViewProps?.onVersionSelect(version, timestamps);
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
    const tab = key as TabKey;
    setActiveTab(tab);
    if (remembersActiveTab) {
      writeStoredValue(ACTIVE_TAB_STORAGE_KEY, tab);
    }
  }, [remembersActiveTab]);

  // The website build shows the repository's mod and nothing of a machine.
  const state: ModDetailsState = extensionViewProps?.state ?? {
    installed: null,
    shown: { kind: 'latest' },
  };
  const { installed, shown } = state;

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

  if (shown.kind === 'installed') {
    const hasLogging = installed?.config?.loggingEnabled ||
      installed?.config?.debugLoggingEnabled;
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

  // What the changes tab diffs is the installed source against the one a move
  // would put in its place, so it goes with there being such a version: the
  // offer standing, the version asked for by name, or the version a refusal is
  // holding off - which is the one a reader weighing whether to take the refusal
  // back is deciding about, and so the one it is worth most reading there. A
  // refusal with no version behind it has nothing to diff.
  const offerAction = extensionViewProps?.headerActions?.actions.offer;
  if (
    offerAction?.kind === 'update' ||
    (offerAction?.kind === 'allow-updates' && offerAction.refusedVersion)
  ) {
    tabList.push(tabHeader('changes', t('modDetails.changes.title')));
  }

  // A tab that has gone leaves the reader on the mod's details, and leaves them
  // there: holding the choice would put them back on it the moment it returned -
  // the changes tab comes and goes with the offer it belongs to.
  const availableActiveTab = tabList.some((x) => x.key === activeTab)
    ? activeTab
    : 'details';
  if (availableActiveTab !== activeTab) {
    setActiveTab(availableActiveTab);
  }

  // Clear the navigation callback when not on settings tab
  useEffect(() => {
    if (availableActiveTab !== 'settings') {
      canNavigateAwayRef.current = null;
    }
  }, [availableActiveTab]);

  // The version list leads to installing or moving to the version it picks, and
  // a screen with neither wired has nothing to pick one for.
  const versionSelectorNode = extensionViewProps?.headerActions && (
    <ModVersionSelector
      isLocalMod={isLocalMod}
      state={state}
      repository={extensionViewProps.repositoryStatus}
      onShowVersion={extensionViewProps.onShowVersion}
      onOpenVersionModal={handleOpenVersionModal}
    />
  );

  // The version on screen, whichever view is naming it. The list opens on it, so
  // a reader sees where they already are among the versions - the installed and
  // the latest version are entries in that list like any other, and only the name
  // the screen shows them under is different.
  const shownVersion =
    shown.kind === 'picked'
      ? shown.version
      : shown.kind === 'installed'
        ? installed?.metadata?.version
        : extensionViewProps?.repositoryStatus?.version;

  return (
    <ModDetailsContainer data-testid="mod-details" data-mod-id={modId}>
      <ModDetailsCard
        title={
          <ModDetailsHeader
            topNode={versionSelectorNode}
            modId={modId}
            modMetadata={modMetadata}
            state={state}
            repositoryDetails={repositoryDetails}
            goBack={goBack}
            extensionHeaderProps={extensionViewProps}
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
          shown={shown}
          activeTab={availableActiveTab}
          modSourceData={modSourceData}
          installedModSourceData={installedModSourceData}
          selectedModSourceData={selectedModSourceData}
          canNavigateAwayRef={canNavigateAwayRef}
          onRetryLoad={onRetryLoad}
        />
      </ModDetailsCard>
      {
        /// #if EXTENSION
        // Opened from the version list, which is drawn on the same terms.
        extensionViewProps?.headerActions && (
          <VersionSelectorModal
            modId={modId}
            open={isVersionModalOpen}
            selectedVersion={shownVersion}
            onSelect={handleVersionSelect}
            onCancel={handleVersionModalCancel}
          />
        )
        /// #endif
      }
    </ModDetailsContainer >
  );
}
