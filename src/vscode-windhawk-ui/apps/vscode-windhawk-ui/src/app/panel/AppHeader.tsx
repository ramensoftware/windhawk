import {
  faCog,
  faHome,
  faInfo,
  faList,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Badge, Button } from 'antd';
import { useCallback, useContext } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';
import styled from 'styled-components';
import { AppUISettingsContext } from '../appUISettings';
import logo from './assets/logo-white.svg';

const Header = styled.header`
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  padding: 20px 20px 0;
  column-gap: 20px;
  margin: 0 auto;
  width: 100%;
  max-width: var(--app-max-width);
`;

const HeaderLogo = styled.div`
  cursor: pointer;
  margin-right: auto;
  font-size: 40px;
  white-space: nowrap;
  font-family: Oxanium;
  user-select: none;
`;

const LogoImage = styled.img`
  height: 80px;
  margin-right: 6px;
`;

const HeaderButtonsWrapper = styled.div`
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin: 12px 0;
`;

const HeaderIcon = styled(FontAwesomeIcon)`
  margin-right: 8px;
`;

function AppHeader() {
  const { t } = useTranslation();

  const navigate = useNavigate();
  const replace = useCallback(
    (to: string) => navigate(to, { replace: true }),
    [navigate]
  );

  const location = useLocation();

  const { updateIsAvailable } = useContext(AppUISettingsContext);

  const buttons = [
    {
      text: t('appHeader.home'),
      route: '/',
      icon: faHome,
    },
    {
      text: t('appHeader.explore'),
      route: '/mods-browser',
      icon: faList,
    },
    {
      text: t('appHeader.settings'),
      route: '/settings',
      icon: faCog,
    },
    {
      text: t('appHeader.about'),
      route: '/about',
      icon: faInfo,
      hasBadge: updateIsAvailable,
    },
  ];

  return (
    <Header>
      <HeaderLogo onClick={() => replace('/')}>
        <LogoImage src={logo} alt="logo" /> Windhawk
      </HeaderLogo>
      <HeaderButtonsWrapper>
        {buttons.map(({ text, route, icon, hasBadge }) => (
          <Badge key={route} dot={hasBadge} status="error">
            <Button
              type={location.pathname === route ? 'primary' : 'default'}
              ghost
              onClick={() => replace(route)}
            >
              <HeaderIcon icon={icon} />
              {text}
            </Button>
          </Badge>
        ))}
      </HeaderButtonsWrapper>
    </Header>
  );
}

export default AppHeader;
