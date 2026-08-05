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
  onClose: () => void;
}

/**
 * What one update contains, over the wizard: the entries it adds and the source it
 * changes.
 *
 * Opens on the changelog, which is what a user weighing an update reads first; the
 * diff is the other tab. Wider than the wizard behind it, because that diff wants
 * the room the select list does not.
 */
export function ModUpdateDetailsModal({
  mod,
  source,
  onLoadInstalledSource,
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
          type="primary"
          data-testid="mod-update-details-close"
          onClick={close}
        >
          {t('general.actions.close')}
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
            children: <ChangesTab mod={mod} source={source} onRetry={onLoadInstalledSource} />,
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
  onRetry,
}: {
  mod: UpdatableMod;
  source: ModUpdateSource | undefined;
  onRetry: (modId: string, force?: boolean) => void;
}) {
  const { t } = useTranslation();

  const newSource = source?.source;
  const oldSource = source?.installedSource;

  if (source?.installedStatus === 'failed') {
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
            onClick={() => onRetry(mod.modId, true)}
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
