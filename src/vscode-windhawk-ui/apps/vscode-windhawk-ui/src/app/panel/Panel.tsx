import React, { useEffect } from 'react';
import { createHashRouter, Outlet, RouterProvider, useNavigate } from 'react-router-dom';
import styled, { css } from 'styled-components';
import About from './About';
import AppHeader from './AppHeader';
import CreateNewModButton from './CreateNewModButton';
import ModPreview from './ModPreview';
import ModsBrowserLocal from './ModsBrowserLocal';
import ModsBrowserOnline from './ModsBrowserOnline';
import SafeModeIndicator from './SafeModeIndicator';
import Settings from './Settings';

const PanelContainer = styled.div`
  display: flex;
  height: 100vh;
  overflow: hidden;
  flex-direction: column;
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
  max-width: var(--app-max-width);
  margin: 0 auto;
  padding: 0 20px;

  // Disable margin-collapsing: https://stackoverflow.com/a/47351270
  display: flex;
  flex-direction: column;
`;

function ContentWrapper({
  ref,
  ...props
}: React.ComponentProps<'div'> & { $hidden?: boolean }) {
  return (
    <ContentContainerScroll ref={ref} {...props}>
      <ContentContainer>{props.children}</ContentContainer>
    </ContentContainerScroll>
  );
}

function ContentWrapperWithOutlet() {
  return (
    <ContentWrapper>
      <Outlet />
    </ContentWrapper>
  );
}

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

function Layout() {
  return (
    <>
      <KeyboardNavigationHandler />
      <SafeModeIndicator />
      <AppHeader />
      <Outlet />
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

const router = createHashRouter([
  {
    element: <Layout />,
    children: [
      {
        path: '/',
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
        path: '/mod-preview/:modId',
        element: <ModPreview ContentWrapper={ContentWrapper} />,
      },
      {
        path: '/mods-browser',
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
        path: '/settings',
        element: <ContentWrapperWithOutlet key="settings" />,
        children: [
          {
            index: true,
            element: <Settings />,
          },
        ],
      },
      {
        path: '/about',
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
]);

function Panel() {
  return (
    <PanelContainer>
      <RouterProvider router={router} />
    </PanelContainer>
  );
}

export default Panel;
