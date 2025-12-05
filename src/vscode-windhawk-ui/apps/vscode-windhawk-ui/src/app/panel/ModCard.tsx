import { faUser } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Badge, Button, Card, Divider, Rate, Switch, Tooltip } from 'antd';
import { useTranslation } from 'react-i18next';
import styled, { css } from 'styled-components';
import EllipsisText from '../components/EllipsisText';
import { PopconfirmModal } from '../components/InputWithContextMenu';
import { ModMetadata, RepositoryDetails } from '../webviewIPCMessages';
import localModIcon from './assets/local-mod-icon.svg';
import ModMetadataLine from './ModMetadataLine';

const ModCardWrapper = styled.div`
  // Fill whole height.
  > .ant-ribbon-wrapper {
    height: 100%;
  }
`;

const ModCardRibbon = styled(Badge.Ribbon) <{ $hidden: boolean }>`
  ${({ $hidden }) =>
    $hidden &&
    css`
      display: none;
    `}
`;

const ModCardWrapperInner = styled(Card)`
  // Fill whole height and stick buttons to the bottom.
  height: 100%;

  > .ant-card-body {
    height: 100%;
    display: flex;
    flex-direction: column;

    > .ant-card-meta {
      flex: 1;
    }
  }
`;

const ModCardTitleContainer = styled.div`
  display: flex;
`;

const ModCardTitle = styled(EllipsisText)`
  flex: 1;
`;

// Used to prevent from the title to overlap with the ribbon.
const ModCardTitleRibbonContent = styled.span`
  position: static;
  margin-inline-end: -16px;
  font-weight: normal;
  visibility: hidden;
`;

const ModLocalIcon = styled.img`
  height: 24px;
  margin-inline-start: 4px;
  cursor: help;
`;

const ModCardActionsContainer = styled.div`
  display: flex;
  align-items: center;
  margin-top: 20px;
  text-align: end;

  > :not(:last-child) {
    margin-inline-end: 10px;
  }

  > :last-child {
    margin-inline-start: auto;
  }
`;

const ModRate = styled(Rate)`
  font-size: 14px;
  pointer-events: none;

  > .ant-rate-star {
    margin-inline-end: 2px;
  }
`;

const RatingBreakdownTooltip = styled.div`
  display: grid;
  grid-template-columns: auto 1fr auto;
  gap: 8px;
  align-items: center;
  min-width: 234px; // Max tooltip width
`;

const BreakdownLine = styled.div`
  display: contents;
`;

const BreakdownStars = styled.span`
  display: flex;
`;

const BreakdownRate = styled(Rate)`
  font-size: 12px;
  pointer-events: none;

  > .ant-rate-star {
    margin-inline-end: 2px;
  }
`;

const BreakdownProgressContainer = styled.div`
  height: 8px;
  background-color: rgba(23, 18, 18, 0.1);
  border-radius: 4px;
`;

const BreakdownProgressBar = styled.div<{ $percentage: number }>`
  height: 100%;
  width: ${(props) => props.$percentage}%;
  background-color: #fadb14;
  border-radius: 4px;
  animation: progressBarFill 0.3s ease;

  @keyframes progressBarFill {
    from {
      width: 0%;
    }
  }
`;

const BreakdownCount = styled.span`
  color: rgba(255, 255, 255, 0.85);
  text-align: end;
  font-size: 12px;
  white-space: nowrap;
`;

interface Props {
  ribbonText?: string;
  title: string;
  isLocal?: boolean;
  description?: string;
  modMetadata?: ModMetadata;
  repositoryDetails?: RepositoryDetails;
  buttons: {
    text: React.ReactNode;
    confirmText?: string;
    confirmOkText?: string;
    confirmCancelText?: string;
    confirmIsDanger?: boolean;
    onClick: () => void;
    badge?: {
      tooltip?: string;
    };
  }[];
  switch?: {
    title?: string;
    checked?: boolean;
    disabled?: boolean;
    onChange: (checked: boolean) => void;
  };
}

function ModCard(props: Props) {
  const { t } = useTranslation();

  // Derive stats from repositoryDetails if available
  const stats = props.repositoryDetails ? {
    users: props.repositoryDetails.users,
    rating: props.repositoryDetails.rating,
    ratingBreakdown: props.repositoryDetails.ratingBreakdown,
  } : null;

  const renderRatingTooltip = () => {
    if (!stats) {
      return t('mod.notRated');
    }

    // Calculate total users for percentage
    const totalUsers = stats.ratingBreakdown.reduce(
      (sum, count) => sum + count,
      0
    );

    if (totalUsers === 0) {
      return t('mod.notRated');
    }

    return (
      <RatingBreakdownTooltip>
        {[5, 4, 3, 2, 1].map((stars) => {
          const count = stats.ratingBreakdown[stars - 1] ?? 0;
          const percentage = (count / totalUsers) * 100;
          return (
            <BreakdownLine key={stars}>
              <BreakdownStars>
                <BreakdownRate disabled value={stars} />
              </BreakdownStars>
              <BreakdownProgressContainer>
                <BreakdownProgressBar $percentage={percentage} />
              </BreakdownProgressContainer>
              <BreakdownCount>
                {t('mod.users', {
                  count,
                  formattedCount: count.toLocaleString(),
                })}
              </BreakdownCount>
            </BreakdownLine>
          );
        })}
      </RatingBreakdownTooltip>
    );
  };

  return (
    <ModCardWrapper>
      <ModCardRibbon text={props.ribbonText} $hidden={!props.ribbonText}>
        <ModCardWrapperInner size="small">
          <Card.Meta
            title={
              <>
                <ModCardTitleContainer>
                  <ModCardTitle tooltipPlacement="bottom">
                    {props.title}
                  </ModCardTitle>
                  {props.isLocal && (
                    <Tooltip title={t('mod.editedLocally')} placement="bottom">
                      <ModLocalIcon src={localModIcon} />
                    </Tooltip>
                  )}
                  {props.ribbonText && (
                    // Used to prevent from the title to overlap with the ribbon.
                    <ModCardTitleRibbonContent className="ant-ribbon">
                      {props.ribbonText}
                    </ModCardTitleRibbonContent>
                  )}
                </ModCardTitleContainer>
                {props.modMetadata && (
                  <ModMetadataLine
                    modMetadata={props.modMetadata}
                    singleLine={true}
                    repositoryDetails={props.repositoryDetails}
                  />
                )}
              </>
            }
            description={props.description || <i>{t('mod.noDescription')}</i>}
          />
          <ModCardActionsContainer>
            {props.buttons.map((button, i) => {
              const buttonElement = button.confirmText ? (
                <PopconfirmModal
                  key={i}
                  placement="bottom"
                  title={button.confirmText}
                  okText={button.confirmOkText}
                  cancelText={button.confirmCancelText}
                  okButtonProps={{ danger: button.confirmIsDanger }}
                  onConfirm={() => button.onClick()}
                >
                  <Button type="default" ghost>
                    {button.text}
                  </Button>
                </PopconfirmModal>
              ) : (
                <Button key={i} type="default" ghost onClick={button.onClick}>
                  {button.text}
                </Button>
              );

              if (button.badge) {
                return (
                  <Badge
                    key={i}
                    dot
                    title={button.badge.tooltip}
                    status="warning"
                  >
                    {buttonElement}
                  </Badge>
                );
              }

              return buttonElement;
            })}
            {props.switch && (
              <Tooltip title={props.switch.title} placement="bottom">
                <Switch
                  checked={props.switch.checked}
                  disabled={props.switch.disabled}
                  onChange={(checked) => props.switch?.onChange(checked)}
                />
              </Tooltip>
            )}
            {stats && (
              <div>
                <FontAwesomeIcon icon={faUser} />{' '}
                {t('mod.users', {
                  count: stats.users,
                  formattedCount: stats.users.toLocaleString(),
                })}
                <Divider type="vertical" />
                <Tooltip title={renderRatingTooltip()} placement="bottom">
                  <span>
                    <ModRate
                      disabled
                      allowHalf
                      defaultValue={stats.rating / 2}
                    />
                  </span>
                </Tooltip>
              </div>
            )}
          </ModCardActionsContainer>
        </ModCardWrapperInner>
      </ModCardRibbon>
    </ModCardWrapper>
  );
}

export default ModCard;
