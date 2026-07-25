import React, { useEffect } from 'react';
import { createBrowserRouter, createHashRouter, Navigate, Outlet, RouterProvider, useNavigate } from 'react-router-dom';
import styled, { css } from 'styled-components';
import AppHeader from './AppHeader';
import usePopupDismissOnScroll from './usePopupDismissOnScroll';
import { ModsBrowserOnline } from './mods-browser';
/// #if WEBSITE
import AppFooter from './AppFooter';
import WebsiteHome from './WebsiteHome';
import Download from './Download';
import Links from './Links';
/// #else
import { About } from './about';
import { ModPreview, ModsBrowserLocal } from './mods-browser';
import SafeModeIndicator from './SafeModeIndicator';
import { Settings } from './settings';
import { CreateNewModButton } from './shared';
import { InstallDevToolsModal } from './shared/InstallDevToolsModal';
/// #endif
/// #if TAURI
import LogPaneMount from './logpane/LogPaneMount';
/// #endif

declare const WEBPACK_IS_WEBSITE: boolean;
declare const WEBPACK_IS_TAURI: boolean;

const PanelContainer = styled.div`
  display: flex;
  height: 100vh; /* Fallback for older browsers */
  height: 100dvh;
  overflow: hidden;
  flex-direction: column;
`;

// The main content region above the log pane (Tauri only). It fills the height the
// pane leaves, and clips so its own inner scroll regions - not the whole column -
// take up the slack.
const MainArea = styled.div`
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
`;

const ContentContainerScroll = styled.div<{ $hidden?: boolean }>`
  ${({ $hidden }) => css`
    display: ${$hidden ? 'none' : 'flex'};
  `}
  position: relative; // needed by nested elements that use position: absolute
  flex: 1;
  overflow: overlay;
`;

const ContentContainer = styled.div`
  width: 100%;
  height: 100%;
  max-width: var(--whui-max-width);
  margin: 0 auto;
  padding: 0 20px;

  // Disable margin-collapsing: https://stackoverflow.com/a/47351270
  display: flex;
  flex-direction: column;
`;

/// #if WEBSITE
function ContentWrapperBrowser({
  ref,
  ...props
}: React.ComponentProps<'div'> & { $hidden?: boolean }) {
  return (
    <ContentContainerScroll ref={ref} {...props}>
      <ContentContainer>
        {props.children}
        <AppFooter />
      </ContentContainer>
    </ContentContainerScroll>
  );
}
/// #endif

/// #if EXTENSION
function ContentWrapperExtension({
  ref,
  ...props
}: React.ComponentProps<'div'> & { $hidden?: boolean }) {
  return (
    <ContentContainerScroll ref={ref} {...props}>
      <ContentContainer>{props.children}</ContentContainer>
    </ContentContainerScroll>
  );
}
/// #endif

const ContentWrapper = WEBPACK_IS_WEBSITE ? ContentWrapperBrowser : ContentWrapperExtension;

function ContentWrapperWithOutlet() {
  return (
    <ContentWrapper>
      <Outlet />
    </ContentWrapper>
  );
}

/// #if EXTENSION
function KeyboardNavigationHandler() {
  const navigate = useNavigate();

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Alt+Left for back navigation
      if (event.altKey && event.key === 'ArrowLeft') {
        event.preventDefault();
        navigate(-1);
      }
      // Alt+Right for forward navigation
      else if (event.altKey && event.key === 'ArrowRight') {
        event.preventDefault();
        navigate(1);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [navigate]);

  return null;
}
/// #endif

/// #if WEBSITE
function LayoutWebsite() {
  return (
    <>
      <AppHeader />
      <Outlet />
    </>
  );
}

const routeConfigWebsite = [
  {
    path: '/',
    element: <LayoutWebsite />,
    children: [
      {
        index: true,
        element: <WebsiteHome ContentWrapper={ContentWrapper} />,
      },
      {
        path: 'mods',
        element: <ModsBrowserOnline ContentWrapper={ContentWrapper} />,
        children: [
          {
            path: ':modId',
            element: null,
          },
        ],
      },
      {
        path: 'links',
        element: (
          <ContentWrapper>
            <Links />
          </ContentWrapper>
        ),
      },
      {
        path: 'download',
        element: (
          <ContentWrapper>
            <Download />
          </ContentWrapper>
        ),
      },
    ],
  },
  {
    path: '*',
    element: <Navigate to="/" replace />,
  },
];

const routerWebsite = createBrowserRouter(routeConfigWebsite);
/// #endif

/// #if EXTENSION
function LayoutExtension() {
  return (
    <>
      <KeyboardNavigationHandler />
      <SafeModeIndicator />
      <AppHeader />
      <Outlet />
      {/* An overlay raised (via the devToolsInstall seam) when a launch entry point
          replies that the development tools are not installed. */}
      <InstallDevToolsModal />
    </>
  );
}

// Must be done before creating the router to ensure the initial route is
// correct.
const bodyParams = document.querySelector('body')?.getAttribute('data-params');
const previewModId = bodyParams && JSON.parse(bodyParams).previewModId;
if (previewModId) {
  const url = new URL(window.location.href);
  url.hash = '#/mod-preview/' + previewModId;
  window.history.replaceState(null, '', url);
}

const routeConfigExtension = [
  {
    path: '/',
    element: <LayoutExtension />,
    children: [
      {
        path: '',
        element: (
          <>
            <ModsBrowserLocal ContentWrapper={ContentWrapper} />
            <CreateNewModButton />
          </>
        ),
        children: [
          {
            path: 'mods/:modType/:modId',
            element: null,
          },
        ],
      },
      {
        path: 'mod-preview/:modId',
        element: <ModPreview ContentWrapper={ContentWrapper} />,
      },
      {
        path: 'mods-browser',
        element: (
          <>
            <ModsBrowserOnline ContentWrapper={ContentWrapper} />
            <CreateNewModButton />
          </>
        ),
        children: [
          {
            path: ':modId',
            element: null,
          },
        ],
      },
      {
        path: 'settings',
        element: <ContentWrapperWithOutlet key="settings" />,
        children: [
          {
            index: true,
            element: <Settings />,
          },
        ],
      },
      {
        path: 'about',
        element: <ContentWrapperWithOutlet key="about" />,
        children: [
          {
            index: true,
            element: <About />,
          },
        ],
      },
    ],
  },
  {
    path: '*',
    element: <Navigate to="/" replace />,
  },
];

const routerExtension = createHashRouter(routeConfigExtension);
/// #endif

const router = WEBPACK_IS_WEBSITE ? routerWebsite : routerExtension;

function Panel() {
  usePopupDismissOnScroll();

  // In the Tauri shell the log pane docks as a resizable bottom split, so the router
  // content is wrapped to fill the space above it. Other builds keep the plain
  // full-height layout (the LogPaneMount import is compiled out for them).
  if (WEBPACK_IS_TAURI) {
    return (
      <PanelContainer>
        <MainArea>
          <RouterProvider router={router} />
        </MainArea>
        <LogPaneMount />
      </PanelContainer>
    );
  }

  return (
    <PanelContainer>
      <RouterProvider router={router} />
    </PanelContainer>
  );
}

export default Panel;
