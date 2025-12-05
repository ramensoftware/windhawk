import {
  faCog,
  faHome,
  faInfo,
  faList,
  IconDefinition,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Badge, Button } from 'antd';
import { PresetStatusColorType } from 'antd/lib/_util/colors';
import { useContext } from 'react';
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
  margin-inline-end: auto;
  font-size: 40px;
  white-space: nowrap;
  font-family: Oxanium;
  user-select: none;
`;

const LogoImage = styled.img`
  height: 80px;
  margin-inline-end: 6px;
`;

const HeaderButtonsWrapper = styled.div`
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin: 12px 0;
`;

const HeaderIcon = styled(FontAwesomeIcon)`
  margin-inline-end: 8px;
`;

type HeaderButton = {
  text: string;
  route: string;
  icon: IconDefinition;
  badge?: {
    status: PresetStatusColorType;
    title?: string;
  };
};

function AppHeader() {
  const { t } = useTranslation();

  const navigate = useNavigate();

  const location = useLocation();

  const { loggingEnabled, updateIsAvailable } = useContext(AppUISettingsContext);

  const buttons: HeaderButton[] = [
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
      badge: loggingEnabled ? {
        status: 'warning',
        title: t('general.loggingEnabled'),
      } : undefined,
    },
    {
      text: t('appHeader.about'),
      route: '/about',
      icon: faInfo,
      badge: updateIsAvailable ? {
        status: 'error',
        title: t('about.update.title'),
      } : undefined,
    },
  ];

  return (
    <Header>
      <HeaderLogo onClick={() => navigate('/')}>
        <LogoImage src={logo} alt="logo" /> Windhawk
      </HeaderLogo>
      <HeaderButtonsWrapper>
        {buttons.map(({ text, route, icon, badge }) => (
          <Badge key={route} dot={!!badge} status={badge?.status} title={badge?.title}>
            <Button
              type={location.pathname === route ? 'primary' : 'default'}
              ghost
              onClick={() => navigate(route)}
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
