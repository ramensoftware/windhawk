import { faUser } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Badge, Button, Card, Checkbox, Divider, Rate, Switch, Tooltip } from 'antd';
import { useTranslation } from 'react-i18next';
import styled, { css } from 'styled-components';
import EllipsisText from '@app/components/EllipsisText';
import { PopconfirmModal } from '@app/components/InputWithContextMenu';
import { type ModMetadata, type RepositoryDetails } from '@app/webviewIPCMessages';
import ButtonLink from './ButtonLink';
import LocalModIcon from './LocalModIcon';
import ModMetadataLine from './ModMetadataLine';
import ModSelectBox, {
  modSelectBoxReach,
  modSelectBoxRevealed,
} from './ModSelectBox';

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

// The room between the card's edge and the line its title is on: antd's body
// padding at size="small". It is what the checkbox travels across.
const CARD_BODY_PADDING = 12;

// What a card that can be selected adds, and nothing more: a card rendered
// without the selection prop - the online browser's, the featured strip's, the
// website's - is laid out and painted exactly as it is with none of this here.
const selectable = css`
  // The checkbox comes out from under the card's own edge, so the title line
  // reaches back across the card's padding and clips there.
  ${ModCardTitleContainer} {
    ${modSelectBoxReach(CARD_BODY_PADDING)}
  }

  // Both of antd's clips between that line and the card sit where the line
  // begins, and either one would cut the box off before it ever got to the edge,
  // so both give way to the one above. What each was worth is put back where it
  // is still wanted: the detail column shrinks past its content on a min-width
  // rather than on its overflow, and the description keeps a clip of its own,
  // since neither has anything to draw out there.
  .ant-card-meta-detail {
    overflow: visible;
    min-width: 0;
  }

  .ant-card-meta-title {
    overflow: visible;
  }

  .ant-card-meta-description {
    overflow: hidden;
  }

  // What brings the checkbox out. Its own hover, the mod being checked, and -
  // read past the card, off the list container - anything at all being checked:
  // selecting is a mode the user is in, whatever the pointer is doing. That last
  // one also settles the names: once a selection is under way every card in the
  // grid has made room, so the line only moves on the way into a selection and
  // on the way out of one, not card by card as the pointer crosses them.
  &:hover ${ModSelectBox},
  &[data-selected] ${ModSelectBox},
  [data-selection-active] & ${ModSelectBox} {
    ${modSelectBoxRevealed}
  }

  // A device with no pointer has nothing to reveal them with, so there they
  // simply stand. Asked as whether hover exists rather than whether the device
  // is a phone: a touch laptop has both a finger and a pointer.
  @media (hover: none) {
    ${ModSelectBox} {
      ${modSelectBoxRevealed}
    }
  }

  // Once the pointer moves on, a 16px checkbox is the only mark a selected mod
  // carries, which is too thin to confirm a removal of eight against. A gradient
  // rather than a background color: the tint is translucent, and a translucent
  // background color would replace the card's own and let the page show through
  // instead of tinting it. The border changes color only - a width change would
  // move every pixel of the card's content inward as it was selected.
  &[data-selected] ${ModCardWrapperInner} {
    background-image: linear-gradient(
      var(--whui-selected-bg),
      var(--whui-selected-bg)
    );
    border-color: var(--whui-primary);
  }
`;

const ModCardWrapper = styled.div<{ $selectable?: boolean }>`
  // Fill whole height.
  > .ant-ribbon-wrapper {
    height: 100%;
  }

  ${({ $selectable }) => $selectable && selectable}
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

const ModLocalIcon = styled(LocalModIcon)`
  margin-inline-start: 4px;
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
  background-color: var(--whui-track-bg);
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
  text-align: end;
  font-size: 12px;
  white-space: nowrap;
`;

// Common button properties
type BaseButton = {
  text: React.ReactNode;
  onClick?: () => void;  // Optional for both modes (href still navigates, onClick for side effects)
  testId?: string;
  badge?: {
    tooltip?: string;
  };
};

// Action type discriminated union
type ButtonAction =
  | { type: 'navigate'; href: string }
  | {
      type: 'confirm';
      confirmText: string;
      confirmOkText?: string;
      confirmCancelText?: string;
      confirmIsDanger?: boolean;
    }
  | { type: 'action' };  // Simple onClick without confirmation

type ModCardButton = BaseButton & ButtonAction;

interface Props {
  modId: string;
  ribbonText?: string;
  title: string;
  isLocal?: boolean;
  description?: string;
  modMetadata?: ModMetadata;
  repositoryDetails?: RepositoryDetails;
  buttons: ModCardButton[];
  switch?: {
    title?: string;
    checked?: boolean;
    disabled?: boolean;
    onChange: (checked: boolean) => void;
  };
  // Absent, the card carries no checkbox and is laid out exactly as it is
  // without this prop at all - which is what every caller that does not select
  // gets. Clicking the card body is deliberately not a second way to toggle: the
  // body already carries Details, Remove and the enable switch, so a click that
  // sometimes selects and sometimes acts is one the user cannot predict.
  selection?: {
    checked: boolean;
    // shiftKey comes from the checkbox's native event, for the range select.
    onChange: (checked: boolean, shiftKey: boolean) => void;
    // The accessible name, composed by the caller from the mod's name.
    label: string;
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
    <ModCardWrapper
      data-testid="mod-card"
      data-mod-id={props.modId}
      data-selected={props.selection?.checked ? '' : undefined}
      $selectable={!!props.selection}
    >
      <ModCardRibbon text={props.ribbonText} $hidden={!props.ribbonText}>
        <ModCardWrapperInner size="small">
          <Card.Meta
            title={
              <>
                <ModCardTitleContainer data-testid="mod-card-title">
                  {props.selection && (
                    <ModSelectBox>
                      <Checkbox
                        data-testid="mod-card-select"
                        aria-label={props.selection.label}
                        checked={props.selection.checked}
                        onChange={(e) =>
                          props.selection?.onChange(
                            e.target.checked,
                            e.nativeEvent.shiftKey
                          )
                        }
                      />
                    </ModSelectBox>
                  )}
                  <ModCardTitle tooltipPlacement="bottom">
                    {props.title}
                  </ModCardTitle>
                  {props.isLocal && (
                    <Tooltip title={t('mod.editedLocally')} placement="bottom">
                      <ModLocalIcon aria-label={t('mod.editedLocally')} />
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
              // Render button based on action type
              let buttonElement: React.ReactNode;

              switch (button.type) {
                case 'navigate':
                  buttonElement = (
                    <ButtonLink
                      key={i}
                      type="default"
                      ghost
                      to={button.href}
                      data-testid={button.testId}
                      onClick={button.onClick}
                    >
                      {button.text}
                    </ButtonLink>
                  );
                  break;

                case 'confirm':
                  buttonElement = (
                    <PopconfirmModal
                      key={i}
                      placement="bottom"
                      title={button.confirmText}
                      okText={button.confirmOkText}
                      cancelText={button.confirmCancelText}
                      okButtonProps={{ danger: button.confirmIsDanger }}
                      onConfirm={() => button.onClick?.()}
                    >
                      <Button type="default" ghost data-testid={button.testId}>
                        {button.text}
                      </Button>
                    </PopconfirmModal>
                  );
                  break;

                case 'action':
                  buttonElement = (
                    <Button
                      key={i}
                      type="default"
                      ghost
                      data-testid={button.testId}
                      onClick={button.onClick}
                    >
                      {button.text}
                    </Button>
                  );
                  break;
              }

              // Wrap in badge if needed
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
                  data-testid="mod-card-switch"
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
                    <ModRate disabled allowHalf value={stats.rating / 2} />
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
