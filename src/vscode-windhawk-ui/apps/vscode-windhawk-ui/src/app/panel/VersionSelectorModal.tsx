import { Badge, Menu, Modal, Spin } from 'antd';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { useGetModVersions } from '../webviewIPC';
import { mockModVersions } from './mockData';

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
  border: 1px solid #303030;

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
  color: var(--vscode-descriptionForeground, #9d9d9d);
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

  // IPC hook for fetching versions
  const { getModVersions, getModVersionsPending } = useGetModVersions(
    useCallback(
      (data) => {
        if (data.modId === props.modId) {
          setVersions(data.versions);
          setLoadedModId(data.modId);
        }
      },
      [props.modId]
    )
  );

  // Fetch versions when modal opens (only if not already loaded for this modId)
  useEffect(() => {
    if (props.open && loadedModId !== props.modId) {
      if (mockModVersions) {
        setVersions(mockModVersions);
        setLoadedModId(props.modId);
      } else {
        getModVersions({ modId: props.modId });
      }
    }
  }, [props.open, props.modId, loadedModId, getModVersions]);

  // Pre-select the version when modal opens
  useEffect(() => {
    if (props.open && props.selectedVersion) {
      setSelectedVersion(props.selectedVersion);
    }
  }, [props.open, props.selectedVersion]);

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
      onOk={handleSelect}
      onCancel={handleCancel}
      okText={t('modDetails.version.select')}
      cancelText={t('modDetails.version.cancel')}
      okButtonProps={{ disabled: !selectedVersion }}
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
