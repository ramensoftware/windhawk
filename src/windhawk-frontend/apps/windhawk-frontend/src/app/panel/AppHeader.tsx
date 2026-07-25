import { AppUISettingsContext } from '@app/appUISettings';
import {
  faCog,
  faHome,
  faInfo,
  faLanguage,
  faList,
  faMoon,
  faSun,
  type IconDefinition,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Badge, Button, Dropdown } from 'antd';
import { type PresetStatusColorType } from 'antd/lib/_util/colors';
import { useContext } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useLocation, useNavigate } from 'react-router-dom';
import styled from 'styled-components';
import logo from './assets/logo-white.svg';
/// #if WEBSITE
import { appLanguages } from '@app/constants/languages';
import { setLanguage } from '@app/i18n';
import { useTheme } from '@app/theme';
import ButtonLink from './shared/ButtonLink';
/// #endif

const Header = styled.header`
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  padding: 20px 20px 0;
  column-gap: 20px;
  margin: 0 auto;
  width: 100%;
  max-width: var(--whui-max-width);
`;

const HeaderLogo = styled.div<{ $cursorPointer?: boolean }>`
  ${({ $cursorPointer: $clickable }) => $clickable && 'cursor: pointer;'}
  margin-inline-end: auto;
  font-size: 40px;
  white-space: nowrap;
  font-family: Oxanium;
  user-select: none;
  color: var(--whui-logo-color);
`;

// The logo is a monochrome silhouette rendered via a CSS mask so it can be
// tinted with `currentColor`, letting it follow the surrounding text color
// (including the theme and hover color) the same way the adjacent wordmark does.
// An <img> can't inherit `color`, so it would stay a fixed white.
const LogoMark = styled.span`
  display: inline-block;
  width: 80px;
  height: 80px;
  vertical-align: middle;
  margin-inline-end: 6px;
  background-color: currentColor;
  -webkit-mask: url(${logo}) center / contain no-repeat;
  mask: url(${logo}) center / contain no-repeat;

  // Forced colors (Windows High Contrast) rewrites background-color to Canvas,
  // which paints the mask in the page background and erases the logo. The
  // system color keywords are honored as authored, so name one explicitly; it
  // has to match what the adjacent wordmark is forced to, which is the page
  // text color here and LinkText inside the link below.
  @media (forced-colors: active) {
    background-color: CanvasText;
  }
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

const HeaderLogoLink = styled(Link)`
  color: var(--whui-logo-color);

  &:hover {
    color: var(--whui-logo-color-hover);
  }

  @media (forced-colors: active) {
    ${LogoMark} {
      background-color: LinkText;
    }
  }
`;

const LogoTextHidden = styled.span`
  visibility: hidden;
`;

/// #if EXTENSION
type HeaderButton = {
  text: string;
  route: string;
  icon: IconDefinition;
  testId: string;
  badge?: {
    status: PresetStatusColorType;
    title?: string;
  };
};

function AppHeaderExtension() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const { loggingEnabled, updateIsAvailable } = useContext(AppUISettingsContext);

  const buttons: HeaderButton[] = [
    {
      text: t('appHeader.home'),
      route: '/',
      icon: faHome,
      testId: 'nav-home',
    },
    {
      text: t('appHeader.explore'),
      route: '/mods-browser',
      icon: faList,
      testId: 'nav-explore',
    },
    {
      text: t('appHeader.settings'),
      route: '/settings',
      icon: faCog,
      testId: 'nav-settings',
      badge: loggingEnabled ? {
        status: 'warning',
        title: t('general.status.loggingEnabled'),
      } : undefined,
    },
    {
      text: t('appHeader.about'),
      route: '/about',
      icon: faInfo,
      testId: 'nav-about',
      badge: updateIsAvailable ? {
        status: 'error',
        title: t('about.update.title'),
      } : undefined,
    },
  ];

  return (
    <Header data-testid="app-header">
      <HeaderLogo $cursorPointer onClick={() => navigate('/')}>
        <LogoMark aria-hidden /> Windhawk
      </HeaderLogo>
      <HeaderButtonsWrapper>
        {buttons.map(({ text, route, icon, testId, badge }) => (
          <Badge key={route} dot={!!badge} status={badge?.status} title={badge?.title}>
            <Button
              type={location.pathname === route ? 'primary' : 'default'}
              ghost
              data-testid={testId}
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
/// #endif

/// #if WEBSITE
type HeaderButtonWebsite = {
  text: string;
  route: string;
  icon: IconDefinition;
};

function AppHeaderBrowser() {
  const { t, i18n } = useTranslation();
  const location = useLocation();
  const { resolvedTheme, setTheme } = useTheme();

  const buttons: HeaderButtonWebsite[] = [
    {
      text: t('appHeader.home'),
      route: '/',
      icon: faHome,
    },
    {
      text: t('website.appHeader.mods'),
      route: '/mods',
      icon: faList,
    },
    {
      text: t('website.appHeader.links'),
      route: '/links',
      icon: faInfo,
    },
  ];

  const handleLanguageChange = (languageCode: string) => {
    setLanguage(languageCode);
    localStorage.setItem('windhawk-language', languageCode);
  };

  const languageMenuItems = [
    ...appLanguages.map(([code, name]) => ({
      key: code,
      label: name,
    })),
    { type: 'divider' as const },
    {
      key: 'contribute',
      label: (
        <a href="https://github.com/ramensoftware/windhawk/wiki/Translations">
          {t('website.appHeader.contributeTranslation')}
        </a>
      ),
    },
  ];

  return (
    <Header>
      <HeaderLogo>
        {location.pathname === '/' ? (
          <>
            <HeaderLogoLink to="/">
              <LogoMark role="img" aria-label="Windhawk" />
            </HeaderLogoLink>
            {/* A hidden text that serves as a layout placeholder */}
            <LogoTextHidden> Windhawk</LogoTextHidden>
          </>
        ) : (
          <HeaderLogoLink to="/">
            <LogoMark aria-hidden /> Windhawk
          </HeaderLogoLink>
        )}
      </HeaderLogo>
      <HeaderButtonsWrapper>
        {buttons.map(({ text, route, icon }) => (
          <ButtonLink
            key={route}
            to={route}
            type={location.pathname.replace(/\/+$/, '') === route.replace(/\/+$/, '') ? 'primary' : 'default'}
            ghost
          >
            <HeaderIcon icon={icon} />
            {text}
          </ButtonLink>
        ))}
        <Dropdown
          menu={{
            style: { maxHeight: '400px', overflowY: 'auto' },
            items: languageMenuItems,
            selectedKeys: [i18n.language],
            onClick: ({ key }) => {
              if (key !== 'contribute') {
                handleLanguageChange(key);
              }
            },
          }}
          trigger={['click']}
        >
          <Button
            ghost
            title={t('website.appHeader.language')}
            aria-label={t('website.appHeader.language')}
          >
            <FontAwesomeIcon icon={faLanguage} />
          </Button>
        </Dropdown>
        <Button
          ghost
          title={t('website.appHeader.toggleTheme')}
          aria-label={t('website.appHeader.toggleTheme')}
          onClick={() => setTheme(resolvedTheme === 'light' ? 'dark' : 'light')}
        >
          <FontAwesomeIcon icon={resolvedTheme === 'light' ? faMoon : faSun} />
        </Button>
      </HeaderButtonsWrapper>
    </Header>
  );
}
/// #endif

declare const WEBPACK_IS_WEBSITE: boolean;

function AppHeader() {
  return WEBPACK_IS_WEBSITE ? <AppHeaderBrowser /> : <AppHeaderExtension />;
}

export default AppHeader;
