import { faGithubAlt } from '@fortawesome/free-brands-svg-icons';
import {
  faArrowLeft,
  faBullhorn,
  faCrosshairs,
  faHome,
  faUser,
  IconDefinition,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Alert, Button, Card, Divider, Modal, Rate, Tooltip } from 'antd';
import { useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { PopconfirmModal } from '../components/InputWithContextMenu';
import { ModMetadata } from '../webviewIPCMessages';
import DevModeAction from './DevModeAction';

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
    margin-right: 12px;
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
  margin-bottom: 4px;

  > * {
    text-overflow: ellipsis;
    overflow: hidden;
  }

  > :not(:first-child) {
    font-size: 14px;
    font-weight: normal;
  }
`;

const CardTitleMetadataLine = styled.div`
  display: flex;
  flex-wrap: wrap;
  margin-bottom: 2px;

  > * {
    font-size: 14px;
    font-weight: normal;
    text-overflow: ellipsis;
    overflow: hidden;
  }
`;

const CardTitleModId = styled.div`
  border-radius: 2px;
  background: #444;
  padding: 0 4px;
`;

const CardTitleMetadataIcon = styled(FontAwesomeIcon)`
  margin-right: 3px;
`;

const CardTitleDescription = styled.div`
  color: rgba(255, 255, 255, 0.45);
  font-size: 14px;
  font-weight: normal;
  text-overflow: ellipsis;
  overflow: hidden;
`;

const ModRate = styled(Rate)`
  line-height: 0.7;
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
            <a href={modMetadata.homepage}>{modMetadata.homepage}</a>
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
            <a href={modMetadata.github}>
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
            <a href={modMetadata.twitter}>
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
  modStatus: ModStatus;
  updateAvailable: boolean;
  installedVersionIsLatest: boolean;
  userRating?: number;
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
  };
}

function ModDetailsHeader(props: Props) {
  const { t } = useTranslation();

  const { modId, modMetadata, modStatus, callbacks } = props;

  let displayModId = props.modId;
  let isLocalMod = false;
  if (modId.startsWith('local@')) {
    displayModId = modId.slice('local@'.length);
    isLocalMod = true;
  }

  const displayModName = modMetadata.name || displayModId;

  const cardMetadataItems: {
    key: string;
    icon: IconDefinition;
    text: string;
    tooltip: string | React.ReactNode;
  }[] = [];

  if (modMetadata.version) {
    cardMetadataItems.push({
      key: 'version',
      icon: faBullhorn,
      text: modMetadata.version,
      tooltip: t('modDetails.header.modVersion'),
    });
  }

  if (modMetadata.author) {
    const authorTooltip = (
      <>
        <div>{t('modDetails.header.modAuthor.title')}</div>
        {(modMetadata.homepage ||
          modMetadata.github ||
          modMetadata.twitter) && (
            <div>
              {modMetadata.homepage && (
                <Tooltip
                  title={t('modDetails.header.modAuthor.homepage')}
                  placement="bottom"
                >
                  <Button
                    type="text"
                    icon={<FontAwesomeIcon icon={faHome} />}
                    href={modMetadata.homepage}
                  />
                </Tooltip>
              )}
              {modMetadata.github && (
                <Tooltip
                  title={t('modDetails.header.modAuthor.github')}
                  placement="bottom"
                >
                  <Button
                    type="text"
                    icon={<FontAwesomeIcon icon={faGithubAlt} />}
                    href={modMetadata.github}
                  />
                </Tooltip>
              )}
              {modMetadata.twitter && (
                <Tooltip
                  title={t('modDetails.header.modAuthor.twitter')}
                  placement="bottom"
                >
                  <Button
                    type="text"
                    icon={<TextAsIconWrapper>𝕏</TextAsIconWrapper>}
                    href={modMetadata.twitter}
                  />
                </Tooltip>
              )}
            </div>
          )}
      </>
    );

    cardMetadataItems.push({
      key: 'author',
      icon: faUser,
      text: modMetadata.author,
      tooltip: authorTooltip,
    });
  }

  if (modMetadata.include && modMetadata.include.length > 0) {
    const include = modMetadata.include;
    const exclude = modMetadata.exclude || [];
    let text: string;
    let tooltip: string;

    if (include.length === 1 && exclude.length === 0) {
      if (include[0] === '*') {
        text = t('modDetails.header.processes.all');
      } else {
        text = include[0];
      }

      tooltip = t('modDetails.header.processes.tooltip.target');
    } else {
      if (include.length === 1 && include[0] === '*') {
        text = t('modDetails.header.processes.allBut', {
          list: exclude.join(', '),
        });
      } else if (exclude.length > 0) {
        text = t('modDetails.header.processes.except', {
          included: include.join(', '),
          excluded: exclude.join(', '),
        });
      } else {
        text = include.join(', ');
      }

      tooltip =
        t('modDetails.header.processes.tooltip.targets') +
        '\n' +
        modMetadata.include.join('\n');

      if (exclude.length > 0) {
        tooltip +=
          '\n' +
          t('modDetails.header.processes.tooltip.excluded') +
          '\n' +
          exclude.join('\n');
      }
    }

    cardMetadataItems.push({
      key: 'processes',
      icon: faCrosshairs,
      text,
      tooltip,
    });
  }

  const [isInstallModalOpen, setIsInstallModalOpen] = useState(false);

  return (
    <ModDetailsHeaderWrapper>
      <Button
        type="text"
        icon={<FontAwesomeIcon icon={faArrowLeft} />}
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
              {cardMetadataItems.length > 0 && (
                <CardTitleMetadataLine>
                  {cardMetadataItems.map((item, i) => (
                    <div key={item.key}>
                      <Tooltip
                        overlayStyle={{ whiteSpace: 'pre-line' }}
                        title={item.tooltip}
                        placement="bottom"
                      >
                        <CardTitleMetadataIcon icon={item.icon} /> {item.text}
                      </Tooltip>
                      {i < cardMetadataItems.length - 1 && (
                        <Divider type="vertical" />
                      )}
                    </div>
                  ))}
                </CardTitleMetadataLine>
              )}
              {modMetadata.description && (
                <CardTitleDescription>
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
                    <Button
                      type="primary"
                      size="small"
                      disabled={
                        !callbacks.updateMod || props.installedVersionIsLatest
                      }
                      onClick={() => callbacks.updateMod?.()}
                    >
                      {t('modDetails.header.update')}
                    </Button>
                  </Tooltip>
                )}
                {modStatus === 'not-installed' ? (
                  !props.updateAvailable && (
                    <Button
                      type="primary"
                      size="small"
                      disabled={!callbacks.installMod}
                      onClick={() => setIsInstallModalOpen(true)}
                    >
                      {t('modDetails.header.install')}
                    </Button>
                  )
                ) : modStatus === 'installed-not-compiled' ? (
                  <Button
                    type="primary"
                    size="small"
                    onClick={() => callbacks.compileMod()}
                  >
                    {t('modDetails.header.compile')}
                  </Button>
                ) : modStatus === 'enabled' ? (
                  <Button
                    type="primary"
                    size="small"
                    onClick={() => callbacks.enableMod(false)}
                  >
                    {t('modDetails.header.disable')}
                  </Button>
                ) : modStatus === 'disabled' ? (
                  <Button
                    type="primary"
                    size="small"
                    onClick={() => callbacks.enableMod(true)}
                  >
                    {t('modDetails.header.enable')}
                  </Button>
                ) : (
                  ''
                )}
                {isLocalMod && (
                  <DevModeAction
                    popconfirmPlacement="bottom"
                    onClick={() => callbacks.editMod()}
                    renderButton={(onClick) => (
                      <Button type="primary" size="small" onClick={onClick}>
                        {t('modDetails.header.edit')}
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
                          {t('modDetails.header.fork')}
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
                        {t('modDetails.header.remove')}
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
                        {t('modDetails.header.fork')}
                      </Button>
                    )}
                  />
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
