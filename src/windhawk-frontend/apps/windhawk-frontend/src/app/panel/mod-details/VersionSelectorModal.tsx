import { Badge, Menu, Modal, Spin } from 'antd';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { testIdProps } from '@app/utils';
import { useGetModVersions } from '@app/webviewIPC';

type ModVersionInfo = {
  version: string;
  timestamp: number;
  isPreRelease: boolean;
};

const ModalContent = styled.div`
  display: flex;
  flex-direction: column;
  gap: 16px;
`;

const MenuWrapper = styled.div`
  max-height: 400px;
  overflow-y: auto;
  border: 1px solid var(--whui-border);

  .ant-menu {
    border: none;
    border-radius: 2px;

    .ant-menu-item {
      margin: 0;
    }
  }
`;

const VersionItemContainer = styled.div`
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
`;

const VersionText = styled.span`
  flex: 1;
`;

const VersionDate = styled.span`
  color: var(--whui-text-muted);
  font-size: 12px;
  margin-inline-start: 8px;
`;

const PreReleaseBadge = styled(Badge)`
  .ant-badge-count {
    background-color: #faad14;
    color: #000;
    font-size: 11px;
  }
`;

interface Props {
  modId: string;
  open: boolean;
  selectedVersion?: string | null;
  onSelect: (version: string, versionTimestamps: Record<string, number>) => void;
  onCancel: () => void;
}

export function VersionSelectorModal(props: Props) {
  const { t } = useTranslation();
  const [selectedVersion, setSelectedVersion] = useState<string | undefined>();
  const [versions, setVersions] = useState<ModVersionInfo[] | null>(null);
  const [loadedModId, setLoadedModId] = useState<string | null>(null);
  const [wasOpen, setWasOpen] = useState(false);

  // Seed the local selection from props when the modal transitions to open. It
  // is what the list opens on every time, a version and the absence of one
  // alike: seeding only from a version would leave the one picked last time
  // standing over a screen that is no longer on any of them.
  if (props.open !== wasOpen) {
    setWasOpen(props.open);
    if (props.open) {
      setSelectedVersion(props.selectedVersion ?? undefined);
    }
  }

  // IPC hook for fetching versions
  const { getModVersions, getModVersionsPending } = useGetModVersions();

  // What a list that lands is judged against, rather than what its request closed
  // over: the mod the dialog is on.
  const modIdRef = useRef(props.modId);
  useEffect(() => {
    modIdRef.current = props.modId;
  });

  // Fetch versions when modal opens (only if not already loaded for this modId)
  const modId = props.modId;
  useEffect(() => {
    if (!props.open || loadedModId === modId) {
      return;
    }

    void (async () => {
      const result = await getModVersions({ modId });
      // Nothing to show from a request the unmount abandoned, or from a list of
      // a mod the dialog has since moved off.
      if (result.status !== 'reply' || modIdRef.current !== modId) {
        return;
      }
      setVersions(result.data.versions);
      setLoadedModId(modId);
    })();
  }, [props.open, modId, loadedModId, getModVersions]);

  const sortedVersions = useMemo(() => {
    if (!versions) {
      return [];
    }
    // Sort by timestamp, newest first
    return [...versions].sort((a, b) => b.timestamp - a.timestamp);
  }, [versions]);

  const formatDate = (timestamp: number) => {
    const date = new Date(timestamp * 1000);
    return date.toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  };

  const handleMenuClick = (version: string) => {
    setSelectedVersion(version);
  };

  const handleSelect = () => {
    if (selectedVersion) {
      const versionTimestamps = versions?.reduce((acc, v) => {
        acc[v.version] = v.timestamp;
        return acc;
      }, {} as Record<string, number>) ?? {};

      props.onSelect(selectedVersion, versionTimestamps);
      setSelectedVersion(undefined);
    }
  };

  const handleCancel = () => {
    setSelectedVersion(undefined);
    props.onCancel();
  };

  const menuItems = useMemo(() => {
    return sortedVersions.map((version) => ({
      key: version.version,
      label: (
        <VersionItemContainer>
          <VersionText>
            {version.version}
            {version.isPreRelease && (
              <>
                {' '}
                <PreReleaseBadge
                  count={t('modDetails.version.preRelease')}
                />
              </>
            )}
          </VersionText>
          <VersionDate>{formatDate(version.timestamp)}</VersionDate>
        </VersionItemContainer>
      ),
    }));
  }, [sortedVersions, t]);

  return (
    <Modal
      open={props.open}
      title={t('modDetails.version.title')}
      onOk={handleSelect}
      onCancel={handleCancel}
      okText={t('modDetails.version.select')}
      cancelText={t('general.actions.cancel')}
      okButtonProps={{
        disabled: !selectedVersion,
        ...testIdProps('version-select-confirm'),
      }}
      centered
      width={360}
      closable={false}
    >
      <ModalContent>
        {getModVersionsPending ? (
          <Spin />
        ) : (
          <MenuWrapper>
            <Menu
              items={menuItems}
              selectedKeys={selectedVersion ? [selectedVersion] : []}
              onClick={({ key }) => handleMenuClick(key)}
            />
          </MenuWrapper>
        )}
      </ModalContent>
    </Modal>
  );
}
