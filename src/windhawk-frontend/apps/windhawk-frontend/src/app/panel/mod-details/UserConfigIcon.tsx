import { CurrentUserContext } from '@app/appUISettings';
import { faUserGear } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Tooltip } from 'antd';
import { useContext } from 'react';
import { useTranslation } from 'react-i18next';
import styled from 'styled-components';

const UserConfigIconWrapper = styled.span.attrs({ role: 'img' })`
  color: var(--whui-primary);
  cursor: help;
`;

// A user icon whose tooltip says the mod's settings and configuration shown
// apply only to the current Windows user, naming the user.
function UserConfigIcon({ className }: { className?: string }) {
  const { t } = useTranslation();
  const currentUser = useContext(CurrentUserContext);

  return (
    // The tooltip renders in a portal, and React bubbles a click on it up the
    // component tree to whatever the icon sits in. A click on the icon itself
    // is in this element's DOM and goes on.
    <span
      onClick={(e) => {
        if (!e.currentTarget.contains(e.target as Node)) {
          e.stopPropagation();
        }
      }}
    >
      <Tooltip
        placement="bottom"
        title={t('modDetails.userConfig.tooltip', {
          userName:
            currentUser?.name ?? t('modDetails.userConfig.unknownUserName'),
        })}
      >
        <UserConfigIconWrapper
          className={className}
          aria-label={t('modDetails.userConfig.label')}
          data-testid="mod-user-config-icon"
        >
          <FontAwesomeIcon icon={faUserGear} />
        </UserConfigIconWrapper>
      </Tooltip>
    </span>
  );
}

export default UserConfigIcon;
