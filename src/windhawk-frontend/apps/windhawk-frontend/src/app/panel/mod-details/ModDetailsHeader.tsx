import EllipsisText from '@app/components/EllipsisText';
import { PopconfirmModal } from '@app/components/InputWithContextMenu';
import { getDisplayModId, sanitizeUrl, testIdProps } from '@app/utils';
import { type ModMetadata, type RepositoryDetails, type UpdateSuppression } from '@app/webviewIPCMessages';
import { faGithubAlt, faXTwitter } from '@fortawesome/free-brands-svg-icons';
import {
  faArrowLeft,
  faArrowRight,
  faHeart,
  faHome,
  faUser,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Alert, Button, Card, ConfigProvider, Dropdown, Modal, Rate, Tooltip } from 'antd';
import React, { Fragment, useContext, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import styled from 'styled-components';
import { DevModeAction, ModMetadataLine } from '../shared';
import type {
  HeaderActions,
  InstalledModAction,
  ModDetailsState,
} from './modDetailsState';

const ModDetailsHeaderWrapper = styled.div`
  display: flex;
  margin-bottom: 4px;

  // https://stackoverflow.com/q/26465745
  .ant-card-meta {
    min-width: 0;
  }
`;

const BackButton = styled(Button)`
  flex-shrink: 0;
  margin-inline-end: 12px;
  // Center vertically with text:
  margin-top: -8px;
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
  background: var(--whui-chip-bg);
  padding: 1px 4px;
`;

const CardTitleDescription = styled(EllipsisText)`
  display: block !important;
  color: var(--whui-text-muted);
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

  // A row with nothing in it is a gap under the description: a preview offers no
  // actions, and a mod with nothing to donate to leaves it empty there.
  &:empty {
    display: none;
  }

  // Fixes a button alignment bug.
  > .ant-tooltip-disabled-compatible-wrapper,
  > .ant-popover-disabled-compatible-wrapper {
    font-size: 0;
  }
`;

// Holds a button to its own width: a split button lays itself out as a block
// and takes the whole row given the chance, and the zeroed font size drops the
// baseline gap that would otherwise misalign it against its neighbors in the
// row.
const CardTitleButtonWrapper = styled.div`
  font-size: 0;
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
            <strong>{t('general.modAuthor.title')}:</strong> {modMetadata.author}
          </div>
        </>
      )}
      {modMetadata.homepage && (
        <>
          <FontAwesomeIcon icon={faHome} />
          <div>
            <strong>{t('general.modAuthor.homepage')}:</strong>{' '}
            <a href={sanitizeUrl(modMetadata.homepage)}>{modMetadata.homepage}</a>
          </div>
        </>
      )}
      {modMetadata.github && (
        <>
          <FontAwesomeIcon icon={faGithubAlt} />
          <div>
            <strong>
              {t('general.modAuthor.github')} (<VerifiedLabel />
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
          <FontAwesomeIcon icon={faXTwitter} />
          <div>
            <strong>
              {t('general.modAuthor.twitter')} (<VerifiedLabel />
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

// The writes the header's own buttons make. Which of them a click can reach is
// the resolver's answer, which reads the same state these were built from, so
// each is called straight rather than asked whether it is there.
export type HeaderCallbacks = {
  installMod: () => void;
  updateMod: () => void;
  // The suppression to store, as the union rather than its stored spelling:
  // the grammar has one implementation, in the contract package.
  disableUpdates: (suppression: UpdateSuppression) => void;
  allowUpdates: () => void;
  forkModFromSource: () => void;
  compileMod: () => void;
  enableMod: (enable: boolean) => void;
  editMod: () => void;
  forkMod: () => void;
  deleteMod: () => void;
  updateModRating: (newRating: number) => void;
};

// The actions the header draws and the writes behind them. One value because
// they answer together: a screen that wires no callbacks resolves no actions,
// and an action with nothing behind it is a button that would sit there forever.
export type HeaderActionsAndCallbacks = {
  actions: HeaderActions;
  callbacks: HeaderCallbacks;
};

// Extension-only header props
export type ExtensionHeaderProps = {
  // Null where the screen's owner wired none of it. The editor's preview is such
  // a screen: every action it could show would report itself unavailable, and
  // the row of them is space that buys nothing. It says nothing about the tabs,
  // which reach the host on their own.
  headerActions: HeaderActionsAndCallbacks | null;
};

/**
 * The row of actions the extension leads with, which the website build has none
 * of.
 *
 * Which actions the mod's state calls for was worked out by the resolver, so
 * every one of them is read off `actions` rather than decided again; what is
 * left here is which control each takes. Drawn only where the callbacks are
 * wired, which is what lets it read them straight rather than asking each one
 * whether it is there.
 */
function ModDetailsHeaderActions(props: {
  actions: HeaderActions;
  callbacks: HeaderCallbacks;
  // The install this confirms names the mod it is putting on the machine.
  modName: string;
  modMetadata: ModMetadata;
}) {
  const { t } = useTranslation();

  const { actions, callbacks, modName, modMetadata } = props;
  const {
    offer: offerAction,
    mod: modAction,
    forkFromSource,
    installed: installedActions,
  } = actions;

  const [isInstallModalOpen, setIsInstallModalOpen] = useState(false);

  // The control each action on the copy on the machine takes. Keyed by the
  // action so the row is drawn in the order the resolver listed them, and so
  // that adding one to that list is a compile error until there is a control for
  // it.
  const installedActionControls: Record<InstalledModAction, React.ReactNode> = {
    edit: (
      <DevModeAction
        popconfirmPlacement="bottom"
        onClick={() => callbacks.editMod()}
        renderButton={({ onClick, loading }) => (
          <Button type="primary" size="small" onClick={onClick} loading={loading}>
            {t('mod.edit')}
          </Button>
        )}
      />
    ),
    fork: (
      <DevModeAction
        popconfirmPlacement="bottom"
        onClick={() => callbacks.forkMod()}
        renderButton={({ onClick, loading }) => (
          <Button type="primary" size="small" onClick={onClick} loading={loading}>
            {t('mod.fork')}
          </Button>
        )}
      />
    ),
    remove: (
      <PopconfirmModal
        placement="bottom"
        title={t('mod.removeConfirm')}
        okText={t('mod.removeConfirmOk')}
        cancelText={t('general.actions.cancel')}
        okButtonProps={{ danger: true }}
        onConfirm={() => callbacks.deleteMod()}
      >
        <Button type="primary" size="small" data-testid="mod-action-remove">
          {t('mod.remove')}
        </Button>
      </PopconfirmModal>
    ),
  };

  // The move names itself after the direction it goes in.
  const updateLabel =
    offerAction?.kind === 'update' && offerAction.downgrade
      ? t('mod.downgrade')
      : t('general.actions.update');

  // What the update action offers besides itself, in the order the menu under it
  // reads: the version the offer brings, and every version.
  const refusableVersion =
    offerAction?.kind === 'update' ? offerAction.refusableVersion : null;
  const refusalMenuItems = refusableVersion
    ? [
        {
          key: 'disable-for-version',
          label: t('modDetails.updates.disableForVersion', {
            version: refusableVersion,
          }),
          onClick: () =>
            callbacks.disableUpdates({
              kind: 'pinned',
              version: refusableVersion,
            }),
        },
        {
          key: 'disable-all',
          label: t('modDetails.updates.disableAll'),
          onClick: () => callbacks.disableUpdates({ kind: 'all' }),
        },
      ]
    : [];

  return (
    <>
      {offerAction?.kind === 'allow-updates' && (
        <Button
          type="primary"
          size="small"
          data-testid="mod-action-allow-updates"
          onClick={() => callbacks.allowUpdates()}
        >
          {t('modDetails.updates.allow')}
        </Button>
      )}
      {offerAction?.kind === 'update' && (
        <CardTitleButtonWrapper data-testid="mod-action-update">
          {refusalMenuItems.length > 0 ? (
            <Dropdown.Button
              type="primary"
              size="small"
              onClick={callbacks.updateMod}
              menu={{ items: refusalMenuItems }}
              buttonsRender={([leftButton, rightButton]) => {
                if (offerAction.blockedBy && React.isValidElement(leftButton)) {
                  return [
                    React.cloneElement(leftButton, { disabled: true }),
                    rightButton,
                  ];
                }
                return [leftButton, rightButton];
              }}
            >
              {updateLabel}
            </Dropdown.Button>
          ) : (
            <Button
              type="primary"
              size="small"
              disabled={!!offerAction.blockedBy}
              onClick={callbacks.updateMod}
            >
              {updateLabel}
            </Button>
          )}
        </CardTitleButtonWrapper>
      )}
      {modAction?.kind === 'install' && (
        <>
          <Button
            type="primary"
            size="small"
            data-testid="mod-action-install"
            disabled={!!modAction.blockedBy}
            onClick={() => setIsInstallModalOpen(true)}
          >
            {t('mod.install')}
          </Button>
          {/* The risk the install carries, weighed against who wrote the mod,
              before the source goes anywhere near the machine. */}
          <Modal
            title={t('installModal.title', { mod: modName })}
            open={isInstallModalOpen}
            centered={true}
            onOk={() => {
              callbacks.installMod();
              setIsInstallModalOpen(false);
            }}
            onCancel={() => {
              setIsInstallModalOpen(false);
            }}
            okText={t('installModal.acceptButton')}
            okButtonProps={{
              disabled: !!modAction.blockedBy,
              ...testIdProps('install-modal-confirm'),
            }}
            cancelText={t('general.actions.cancel')}
          >
            <ModInstallationModalContent data-testid="install-modal">
              <ModInstallationAlert
                message={<h3>{t('installModal.warningTitle')}</h3>}
                description={t('installModal.warningDescription')}
                type="warning"
                showIcon
              />
              <ModInstallationDetailsGrid modMetadata={modMetadata} />
            </ModInstallationModalContent>
          </Modal>
        </>
      )}
      {modAction?.kind === 'compile' && (
        <Button
          type="primary"
          size="small"
          data-testid="mod-action-compile"
          onClick={() => callbacks.compileMod()}
        >
          {t('mod.compile')}
        </Button>
      )}
      {modAction?.kind === 'enable' && (
        <Button
          type="primary"
          size="small"
          data-testid={
            modAction.enable ? 'mod-action-enable' : 'mod-action-disable'
          }
          onClick={() => callbacks.enableMod(modAction.enable)}
        >
          {modAction.enable ? t('mod.enable') : t('mod.disable')}
        </Button>
      )}
      {installedActions.map((installedAction) => (
        <Fragment key={installedAction}>
          {installedActionControls[installedAction]}
        </Fragment>
      ))}
      {forkFromSource && (
        <DevModeAction
          disabled={!!forkFromSource.blockedBy}
          popconfirmPlacement="bottom"
          onClick={() => callbacks.forkModFromSource()}
          renderButton={({ onClick, loading }) => (
            <Button
              type="primary"
              size="small"
              disabled={!!forkFromSource.blockedBy}
              onClick={onClick}
              loading={loading}
            >
              {t('mod.fork')}
            </Button>
          )}
        />
      )}
    </>
  );
}

interface Props {
  topNode?: React.ReactNode;
  modId: string;
  modMetadata: ModMetadata;
  // The mod as it sits on the machine, and which of its versions is on screen.
  // The website build has no machine, and its owner says so once.
  state: ModDetailsState;
  repositoryDetails?: RepositoryDetails;
  // Absent where the screen is the whole of what its owner shows, leaving
  // nowhere for the way back to lead.
  goBack?: () => void;

  // Extension-specific props (all grouped together)
  extensionHeaderProps?: ExtensionHeaderProps;
}

function ModDetailsHeader(props: Props) {
  const { t } = useTranslation();

  const { modId, modMetadata, state, repositoryDetails, goBack, extensionHeaderProps } = props;

  // What this screen can do with the mod, and what to run for each. Null for a
  // screen that wires none of it - the rating goes with the rest, being one of
  // the writes a preview leaves out.
  const headerActions = extensionHeaderProps?.headerActions ?? null;

  // The mod's own copy on the machine, which the rating is a value of. Whether
  // it can be rated at all is the resolver's answer above.
  const { installed: installedMod, shown } = state;

  // The config the mod runs with, which describes that copy: a version being
  // read beside it runs with nothing. Absent for a mod never compiled.
  const modConfig =
    (shown.kind === 'installed' && installedMod?.config) || undefined;

  const { direction } = useContext(ConfigProvider.ConfigContext);

  const displayModId = getDisplayModId(modId);

  const displayModName = modMetadata.name || displayModId;

  return (
    <ModDetailsHeaderWrapper>
      {goBack && (
        <BackButton
          type="text"
          icon={<FontAwesomeIcon icon={direction === 'rtl' ? faArrowRight : faArrowLeft} />}
          data-testid="mod-details-back"
          onClick={goBack}
        />
      )}
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
                  patternsMatchCriticalSystemProcesses: modConfig.patternsMatchCriticalSystemProcesses,
                }}
                repositoryDetails={repositoryDetails}
              />
              {modMetadata.description && (
                <CardTitleDescription tooltipPlacement="bottom">
                  {modMetadata.description}
                </CardTitleDescription>
              )}
              {headerActions?.actions.rate && (
                <ModRate
                  value={installedMod?.userRating}
                  onChange={(newRating) =>
                    headerActions.callbacks.updateModRating(newRating)
                  }
                />
              )}
              <CardTitleButtons data-testid="mod-actions">
                {!extensionHeaderProps ? (
                  <Button
                    type="primary"
                    size="small"
                    href="https://ramensoftware.com/downloads/windhawk_setup.exe"
                  >
                    {t('website.modDetails.getWindhawk')}
                  </Button>
                ) : headerActions && (
                  <ModDetailsHeaderActions
                    actions={headerActions.actions}
                    callbacks={headerActions.callbacks}
                    modName={displayModName}
                    modMetadata={modMetadata}
                  />
                )}
                {/* Not an action on the mod but a link away from it, which
                    stands whether or not this screen can act. */}
                {extensionHeaderProps && modMetadata.donateUrl && (
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
    </ModDetailsHeaderWrapper>
  );
}

export default ModDetailsHeader;
