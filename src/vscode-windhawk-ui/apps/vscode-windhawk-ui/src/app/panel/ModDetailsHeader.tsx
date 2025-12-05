import { faGithubAlt } from '@fortawesome/free-brands-svg-icons';
import {
  faArrowLeft,
  faArrowRight,
  faHeart,
  faHome,
  faUser,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Alert, Button, Card, ConfigProvider, Dropdown, Modal, Rate, Tooltip } from 'antd';
import { useContext, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';
import EllipsisText from '../components/EllipsisText';
import { PopconfirmModal } from '../components/InputWithContextMenu';
import { sanitizeUrl } from '../utils';
import { ModConfig, ModMetadata, RepositoryDetails } from '../webviewIPCMessages';
import DevModeAction from './DevModeAction';
import ModMetadataLine from './ModMetadataLine';

const TextAsIconWrapper = styled.span`
  font-size: 18px;
  line-height: 18px;
  user-select: none;
`;

const ModDetailsHeaderWrapper = styled.div`
  display: flex;
  margin-bottom: 4px;

  > :first-child {
    flex-shrink: 0;
    margin-inline-end: 12px;
    // Center vertically with text:
    margin-top: -8px;
  }

  // https://stackoverflow.com/q/26465745
  .ant-card-meta {
    min-width: 0;
  }
`;

const CardTitleWrapper = styled.div`
  padding-bottom: 4px;
`;

const CardTitleFirstLine = styled.div`
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  column-gap: 8px;

  > * {
    text-overflow: ellipsis;
    overflow: hidden;
  }

  > :not(:first-child) {
    font-size: 14px;
    font-weight: normal;
  }
`;

const CardTitleModId = styled.div`
  border-radius: 2px;
  background: #444;
  padding: 0 4px;
`;

const CardTitleDescription = styled(EllipsisText)`
  display: block !important;
  color: rgba(255, 255, 255, 0.45);
  font-size: 14px;
  font-weight: normal;
`;

const ModRate = styled(Rate)`
  line-height: 0.7;
`;

const HeartIcon = styled(FontAwesomeIcon)`
  color: #ff4d4f;
  margin-inline-end: 4px;
`;

const CardTitleButtons = styled.div`
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 8px;

  // Fixes a button alignment bug.
  > .ant-tooltip-disabled-compatible-wrapper,
  > .ant-popover-disabled-compatible-wrapper {
    font-size: 0;
  }
`;

const ModInstallationAlert = styled(Alert)`
  line-height: 1.2;
`;

const ModInstallationModalContent = styled.div`
  display: flex;
  flex-direction: column;
  row-gap: 24px;
`;

const ModInstallationDetails = styled.div`
  display: grid;
  grid-template-columns: 20px auto;
  align-items: center;
  row-gap: 4px;
`;

const ModInstallationDetailsVerified = styled.span`
  text-decoration: underline dotted;
  cursor: help;
`;

export type ModStatus =
  | 'not-installed'
  | 'installed-not-compiled'
  | 'disabled'
  | 'enabled';

function VerifiedLabel() {
  const { t } = useTranslation();

  return (
    <Tooltip
      title={
        <Trans
          t={t}
          i18nKey="installModal.verifiedTooltip"
          components={[<strong />]}
        />
      }
      placement="bottom"
    >
      <ModInstallationDetailsVerified>
        {t('installModal.verified')}
      </ModInstallationDetailsVerified>
    </Tooltip>
  );
}

function ModInstallationDetailsGrid(props: { modMetadata: ModMetadata }) {
  const { t } = useTranslation();

  const { modMetadata } = props;

  return (
    <ModInstallationDetails>
      {modMetadata.author && (
        <>
          <FontAwesomeIcon icon={faUser} />
          <div>
            <strong>{t('installModal.modAuthor')}:</strong> {modMetadata.author}
          </div>
        </>
      )}
      {modMetadata.homepage && (
        <>
          <FontAwesomeIcon icon={faHome} />
          <div>
            <strong>{t('installModal.homepage')}:</strong>{' '}
            <a href={sanitizeUrl(modMetadata.homepage)}>{modMetadata.homepage}</a>
          </div>
        </>
      )}
      {modMetadata.github && (
        <>
          <FontAwesomeIcon icon={faGithubAlt} />
          <div>
            <strong>
              {t('installModal.github')} (<VerifiedLabel />
              ):
            </strong>{' '}
            <a href={sanitizeUrl(modMetadata.github)}>
              {modMetadata.github.replace(
                /^https:\/\/github\.com\/([a-z0-9-]+)$/i,
                '$1'
              )}
            </a>
          </div>
        </>
      )}
      {modMetadata.twitter && (
        <>
          <TextAsIconWrapper>𝕏</TextAsIconWrapper>
          <div>
            <strong>
              {t('installModal.twitter')} (<VerifiedLabel />
              ):
            </strong>{' '}
            <a href={sanitizeUrl(modMetadata.twitter)}>
              {modMetadata.twitter.replace(
                /^https:\/\/(?:twitter|x)\.com\/([a-z0-9_]+)$/i,
                '@$1'
              )}
            </a>
          </div>
        </>
      )}
    </ModInstallationDetails>
  );
}

interface Props {
  topNode?: React.ReactNode;
  modId: string;
  modMetadata: ModMetadata;
  modConfig?: ModConfig;
  modStatus: ModStatus;
  updateAvailable: boolean;
  installedVersionIsLatest: boolean;
  isDowngrade: boolean;
  userRating?: number;
  repositoryDetails?: RepositoryDetails;
  callbacks: {
    goBack: () => void;
    installMod?: () => void;
    updateMod?: () => void;
    forkModFromSource?: () => void;
    compileMod: () => void;
    enableMod: (enable: boolean) => void;
    editMod: () => void;
    forkMod: () => void;
    deleteMod: () => void;
    updateModRating: (newRating: number) => void;
    onOpenVersionModal?: () => void;
  };
}

function ModDetailsHeader(props: Props) {
  const { t } = useTranslation();

  const { modId, modMetadata, modConfig, modStatus, callbacks } = props;

  const { direction } = useContext(ConfigProvider.ConfigContext);

  let displayModId = props.modId;
  let isLocalMod = false;
  if (modId.startsWith('local@')) {
    displayModId = modId.slice('local@'.length);
    isLocalMod = true;
  }

  const displayModName = modMetadata.name || displayModId;

  const [isInstallModalOpen, setIsInstallModalOpen] = useState(false);

  return (
    <ModDetailsHeaderWrapper>
      <Button
        type="text"
        icon={<FontAwesomeIcon icon={direction === 'rtl' ? faArrowRight : faArrowLeft} />}
        onClick={() => callbacks.goBack()}
      />
      <Card.Meta
        title={
          <>
            {props.topNode}
            <CardTitleWrapper>
              <CardTitleFirstLine>
                <div>{displayModName}</div>
                <Tooltip
                  title={t('modDetails.header.modId')}
                  placement="bottom"
                >
                  <CardTitleModId>{displayModId}</CardTitleModId>
                </Tooltip>
              </CardTitleFirstLine>
              <ModMetadataLine
                modMetadata={modMetadata}
                customProcesses={modConfig && {
                  include: modConfig.includeCustom,
                  exclude: modConfig.excludeCustom,
                  includeExcludeCustomOnly: modConfig.includeExcludeCustomOnly,
                }}
                repositoryDetails={props.repositoryDetails}
              />
              {modMetadata.description && (
                <CardTitleDescription tooltipPlacement="bottom">
                  {modMetadata.description}
                </CardTitleDescription>
              )}
              {modStatus !== 'not-installed' &&
                modStatus !== 'installed-not-compiled' &&
                !isLocalMod && (
                  <ModRate
                    value={props.userRating}
                    onChange={(newRating) =>
                      callbacks.updateModRating(newRating)
                    }
                  />
                )}
              <CardTitleButtons>
                {props.updateAvailable && (
                  <Tooltip
                    title={
                      props.installedVersionIsLatest &&
                      t('modDetails.header.updateNotNeeded')
                    }
                    placement="bottom"
                  >
                    {/* Wrap in div to prevent taking 100% width */}
                    <div>
                      <Dropdown.Button
                        type="primary"
                        size="small"
                        disabled={
                          !callbacks.updateMod || props.installedVersionIsLatest
                        }
                        onClick={() => callbacks.updateMod?.()}
                        menu={{
                          items: [
                            {
                              key: 'choose',
                              label: t('modDetails.version.chooseVersion'),
                              onClick: callbacks.onOpenVersionModal,
                            },
                          ],
                        }}
                      >
                        {props.isDowngrade
                          ? t('mod.downgrade')
                          : t('mod.update')}
                      </Dropdown.Button>
                    </div>
                  </Tooltip>
                )}
                {modStatus === 'not-installed' ? (
                  !props.updateAvailable && (
                    // Wrap in div to prevent taking 100% width
                    <div>
                      <Dropdown.Button
                        type="primary"
                        size="small"
                        disabled={!callbacks.installMod}
                        onClick={() => setIsInstallModalOpen(true)}
                        menu={{
                          items: [
                            {
                              key: 'choose',
                              label: t('modDetails.version.chooseVersion'),
                              onClick: callbacks.onOpenVersionModal,
                            },
                          ],
                        }}
                      >
                        {t('mod.install')}
                      </Dropdown.Button>
                    </div>
                  )
                ) : modStatus === 'installed-not-compiled' ? (
                  <Button
                    type="primary"
                    size="small"
                    onClick={() => callbacks.compileMod()}
                  >
                    {t('mod.compile')}
                  </Button>
                ) : modStatus === 'enabled' ? (
                  <Button
                    type="primary"
                    size="small"
                    onClick={() => callbacks.enableMod(false)}
                  >
                    {t('mod.disable')}
                  </Button>
                ) : modStatus === 'disabled' ? (
                  <Button
                    type="primary"
                    size="small"
                    onClick={() => callbacks.enableMod(true)}
                  >
                    {t('mod.enable')}
                  </Button>
                ) : (
                  ''
                )}
                {modStatus !== 'not-installed' &&
                  isLocalMod && (
                    <DevModeAction
                      popconfirmPlacement="bottom"
                      onClick={() => callbacks.editMod()}
                      renderButton={(onClick) => (
                        <Button type="primary" size="small" onClick={onClick}>
                          {t('mod.edit')}
                        </Button>
                      )}
                    />
                  )}
                {modStatus !== 'not-installed' ? (
                  <>
                    <DevModeAction
                      popconfirmPlacement="bottom"
                      onClick={() => callbacks.forkMod()}
                      renderButton={(onClick) => (
                        <Button type="primary" size="small" onClick={onClick}>
                          {t('mod.fork')}
                        </Button>
                      )}
                    />
                    <PopconfirmModal
                      placement="bottom"
                      title={t('mod.removeConfirm')}
                      okText={t('mod.removeConfirmOk')}
                      cancelText={t('mod.removeConfirmCancel')}
                      okButtonProps={{ danger: true }}
                      onConfirm={() => callbacks.deleteMod()}
                    >
                      <Button type="primary" size="small">
                        {t('mod.remove')}
                      </Button>
                    </PopconfirmModal>
                  </>
                ) : (
                  <DevModeAction
                    disabled={!callbacks.forkModFromSource}
                    popconfirmPlacement="bottom"
                    onClick={() => callbacks.forkModFromSource?.()}
                    renderButton={(onClick) => (
                      <Button
                        type="primary"
                        size="small"
                        disabled={!callbacks.forkModFromSource}
                        onClick={onClick}
                      >
                        {t('mod.fork')}
                      </Button>
                    )}
                  />
                )}
                {modMetadata.donateUrl && (
                  <Button
                    type="primary"
                    size="small"
                    href={sanitizeUrl(modMetadata.donateUrl)}
                    target="_blank"
                  >
                    <HeartIcon icon={faHeart} />
                    {t('mod.donate')}
                  </Button>
                )}
              </CardTitleButtons>
            </CardTitleWrapper>
          </>
        }
      />
      <Modal
        title={t('installModal.title', {
          mod: displayModName,
        })}
        open={isInstallModalOpen}
        centered={true}
        onOk={() => {
          callbacks.installMod?.();
          setIsInstallModalOpen(false);
        }}
        onCancel={() => {
          setIsInstallModalOpen(false);
        }}
        okText={t('installModal.acceptButton')}
        okButtonProps={{
          disabled: !callbacks.installMod,
        }}
        cancelText={t('installModal.cancelButton')}
      >
        <ModInstallationModalContent>
          <ModInstallationAlert
            message={<h3>{t('installModal.warningTitle')}</h3>}
            description={t('installModal.warningDescription')}
            type="warning"
            showIcon
          />
          <ModInstallationDetailsGrid modMetadata={modMetadata} />
        </ModInstallationModalContent>
      </Modal>
    </ModDetailsHeaderWrapper>
  );
}

export default ModDetailsHeader;
