import ModDetailsChangelog, {
  ChangelogMarkdown,
} from '@app/panel/mod-details/tabs/ModDetailsChangelog';
import ModDetailsSourceDiff from '@app/panel/mod-details/tabs/ModDetailsSourceDiff';
import useModalClose from '@app/panel/shared/useModalClose';
import { Button, Collapse, Modal, Result, Spin, Tabs } from 'antd';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';

import { changelogSince } from './changelogSince';
import type { UpdatableMod } from './ModUpdateList';
import type { ModUpdateSource } from './useModUpdateSources';

export type ModUpdateDetailsTab = 'changelog' | 'changes';

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

const OlderEntries = styled(Collapse)`
  margin-top: 16px;
`;

interface Props {
  mod: UpdatableMod;
  source: ModUpdateSource | undefined;
  // Asks for the mod's installed source, which only the diff needs. `force` is the
  // retry offered after a failed fetch.
  onLoadInstalledSource: (modId: string, force?: boolean) => void;
  // Asks for the mod's repository source again, for a diff whose fetch of it
  // failed: the wizard prefetched that side, so this modal has no other way to it.
  onRetrySource: (modId: string) => void;
  // Installs this mod's update. The modal reports it and dismisses itself; the
  // run, and everything it says about itself, belong to the wizard behind.
  onUpdate: () => void;
  onClose: () => void;
}

/**
 * What one update contains, over the wizard: the entries it adds and the source it
 * changes - and the way to accept it, for a user who came to read this one mod up
 * and is done once they have.
 *
 * Opens on the changelog, which is what a user weighing an update reads first; the
 * diff is the other tab. Wider than the wizard behind it, because that diff wants
 * the room the select list does not.
 */
export function ModUpdateDetailsModal({
  mod,
  source,
  onLoadInstalledSource,
  onRetrySource,
  onUpdate,
  onClose,
}: Props) {
  const { t } = useTranslation();
  const { open, close, afterClose } = useModalClose(onClose);
  const [activeTab, setActiveTab] = useState<ModUpdateDetailsTab>('changelog');

  useEffect(() => {
    if (activeTab === 'changes') {
      onLoadInstalledSource(mod.modId);
    }
  }, [activeTab, mod.modId, onLoadInstalledSource]);

  // installMod takes the source text, so there is nothing to install until the
  // repository source is in hand, and the button is where that wait shows: the
  // changelog on screen is a fetch of its own and reads the same either way. A
  // source that failed leaves the button dead, and the changes tab is where it
  // is asked for again.
  const sourceReady = source?.status === 'ready' && !!source.source;
  const sourcePending = !source || source.status === 'loading';

  return (
    <Modal
      open={open}
      afterClose={afterClose}
      title={mod.name}
      width={900}
      centered
      onCancel={close}
      wrapProps={{ 'data-testid': 'mod-update-details-modal' }}
      footer={[
        <Button
          key="close"
          data-testid="mod-update-details-close"
          onClick={close}
        >
          {t('general.actions.close')}
        </Button>,
        <Button
          key="update"
          type="primary"
          loading={sourcePending}
          disabled={!sourceReady}
          data-testid="mod-update-details-update"
          onClick={() => {
            onUpdate();
            close();
          }}
        >
          {t('general.actions.update')}
        </Button>,
      ]}
      bodyStyle={{ maxHeight: '70vh', overflow: 'auto' }}
    >
      <Tabs
        activeKey={activeTab}
        onChange={(key) => setActiveTab(key as ModUpdateDetailsTab)}
        items={[
          {
            key: 'changelog',
            label: (
              <span data-testid="mod-update-tab-changelog">
                {t('modDetails.changelog.title')}
              </span>
            ),
            children: (
              <ModDetailsChangelog
                modId={mod.modId}
                loadingNode={
                  <ProgressSpin size="large" tip={t('general.status.loading')} />
                }
                renderMarkdown={(markdown) => (
                  <ChangelogSinceInstalled
                    markdown={markdown}
                    installedVersion={mod.installedVersion}
                  />
                )}
              />
            ),
          },
          {
            key: 'changes',
            label: (
              <span data-testid="mod-update-tab-changes">
                {t('modDetails.changes.title')}
              </span>
            ),
            children: (
              <ChangesTab
                mod={mod}
                source={source}
                onRetrySource={onRetrySource}
                onRetryInstalledSource={(modId) =>
                  onLoadInstalledSource(modId, true)
                }
              />
            ),
          },
        ]}
      />
    </Modal>
  );
}

// The changelog with the entries this update brings first and the ones the user
// already has behind a panel they can open.
function ChangelogSinceInstalled({
  markdown,
  installedVersion,
}: {
  markdown: string;
  installedVersion?: string;
}) {
  const { t } = useTranslation();
  const { newEntries, olderEntries } = changelogSince(markdown, installedVersion);

  return (
    <div data-testid="mod-update-changelog-content">
      <ChangelogMarkdown markdown={newEntries} />
      {olderEntries && (
        <OlderEntries>
          <Collapse.Panel
            key="older"
            header={t('modDetails.changelog.olderEntries')}
          >
            <ChangelogMarkdown markdown={olderEntries} />
          </Collapse.Panel>
        </OlderEntries>
      )}
    </div>
  );
}

function ChangesTab({
  mod,
  source,
  onRetrySource,
  onRetryInstalledSource,
}: {
  mod: UpdatableMod;
  source: ModUpdateSource | undefined;
  onRetrySource: (modId: string) => void;
  onRetryInstalledSource: (modId: string) => void;
}) {
  const { t } = useTranslation();

  const newSource = source?.source;
  const oldSource = source?.installedSource;

  // Either side of the diff can come back without a source, and neither side is
  // told from one still on its way by the source it has: a fetch that failed is
  // settled, so waiting on it is waiting forever. The retry asks for the sides
  // that failed, which can be both at once.
  const sourceFailed = source?.status === 'failed';
  const installedSourceFailed = source?.installedStatus === 'failed';

  if (sourceFailed || installedSourceFailed) {
    return (
      <Result
        status="error"
        title={t('general.status.loadingFailedTitle')}
        subTitle={t('general.status.loadingFailedSubtitle')}
        extra={[
          <Button
            key="try-again"
            type="primary"
            data-testid="mod-update-changes-retry"
            onClick={() => {
              if (sourceFailed) {
                onRetrySource(mod.modId);
              }
              if (installedSourceFailed) {
                onRetryInstalledSource(mod.modId);
              }
            }}
          >
            {t('general.status.tryAgain')}
          </Button>,
        ]}
      />
    );
  }

  if (!newSource || !oldSource) {
    return <ProgressSpin size="large" tip={t('general.status.loading')} />;
  }

  if (oldSource === newSource) {
    return <NoDataMessage>{t('modDetails.changes.noData')}</NoDataMessage>;
  }

  return (
    <div data-testid="mod-update-changes-content">
      <ModDetailsSourceDiff oldSource={oldSource} newSource={newSource} />
    </div>
  );
}

export default ModUpdateDetailsModal;
