import { faUser } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Badge, Button, Card, Divider, Rate, Switch, Tooltip } from 'antd';
import { useTranslation } from 'react-i18next';
import styled, { css } from 'styled-components';
import InputWithContextMenu from '../components/InputWithContextMenu';
import localModIcon from './assets/local-mod-icon.svg';

const ModCardWrapper = styled.div`
  // Fill whole height.
  > .ant-ribbon-wrapper {
    height: 100%;
  }
`;

const ModCardRibbon = styled(Badge.Ribbon)<{ $hidden: boolean }>`
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

const ModCardTitle = styled.span`
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
`;

// Used to prevent from the title to overlap with the ribbon.
const ModCardTitleRibbonContent = styled.span`
  position: static;
  margin-right: -16px;
  font-weight: normal;
  visibility: hidden;
`;

const ModLocalIcon = styled.img`
  height: 24px;
  margin-left: 4px;
  cursor: help;
`;

const ModCardActionsContainer = styled.div`
  display: flex;
  align-items: center;
  margin-top: 20px;

  > :not(:last-child) {
    margin-right: 10px;
  }

  > :last-child {
    margin-left: auto;
  }
`;

const ModRate = styled(Rate)`
  font-size: 14px;
  pointer-events: none;

  > .ant-rate-star {
    margin-right: 2px;
  }
`;

interface Props {
  ribbonText?: string;
  title: string;
  isLocal?: boolean;
  description?: string;
  buttons: {
    text: string;
    confirmText?: string;
    confirmOkText?: string;
    confirmCancelText?: string;
    confirmIsDanger?: boolean;
    onClick: () => void;
  }[];
  switch?: {
    title?: string;
    checked?: boolean;
    disabled?: boolean;
    onChange: (checked: boolean) => void;
  };
  stats?: {
    users: number;
    rating: number;
  };
}

function ModCard(props: Props) {
  const { t } = useTranslation();

  return (
    <ModCardWrapper>
      <ModCardRibbon text={props.ribbonText} $hidden={!props.ribbonText}>
        <ModCardWrapperInner size="small">
          <Card.Meta
            title={
              <ModCardTitleContainer>
                <ModCardTitle>{props.title}</ModCardTitle>
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
            }
            description={props.description || <i>{t('mod.noDescription')}</i>}
          />
          <ModCardActionsContainer>
            {props.buttons.map((button, i) =>
              button.confirmText ? (
                <InputWithContextMenu.Popconfirm
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
                </InputWithContextMenu.Popconfirm>
              ) : (
                <Button key={i} type="default" ghost onClick={button.onClick}>
                  {button.text}
                </Button>
              )
            )}
            {props.switch && (
              <Tooltip title={props.switch.title} placement="bottom">
                <Switch
                  checked={props.switch.checked}
                  disabled={props.switch.disabled}
                  onChange={(checked) => props.switch?.onChange(checked)}
                />
              </Tooltip>
            )}
            {props.stats && (
              <div>
                <FontAwesomeIcon icon={faUser} />{' '}
                {t('mod.users', {
                  count: props.stats.users,
                  formattedCount: props.stats.users.toLocaleString(),
                })}
                <Divider type="vertical" />
                <ModRate
                  disabled
                  allowHalf
                  defaultValue={props.stats.rating / 2}
                />
              </div>
            )}
          </ModCardActionsContainer>
        </ModCardWrapperInner>
      </ModCardRibbon>
    </ModCardWrapper>
  );
}

export default ModCard;
