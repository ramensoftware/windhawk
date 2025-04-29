import React from 'react';
import { HashRouter as Router, Outlet, Route, Routes } from 'react-router-dom';
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

function Panel() {
  return (
    <PanelContainer>
      <Router>
        <SafeModeIndicator />
        <AppHeader />
        <Routes>
          <Route
            path="/"
            element={<ModsBrowserLocal ContentWrapper={ContentWrapper} />}
          >
            <Route path="mods/:modType/:modId" element={null} />
          </Route>
          <Route
            path="mod-preview/:modId"
            element={<ModPreview ContentWrapper={ContentWrapper} />}
          />
          <Route
            path="mods-browser"
            element={<ModsBrowserOnline ContentWrapper={ContentWrapper} />}
          >
            <Route path=":modId" element={null} />
          </Route>
          {/* Without key for ContentWrapper, DOM element might be reused, in which case the scroll doesn't reset */}
          <Route
            path="settings"
            element={<ContentWrapperWithOutlet key="settings" />}
          >
            <Route index element={<Settings />} />
          </Route>
          <Route
            path="about"
            element={<ContentWrapperWithOutlet key="about" />}
          >
            <Route index element={<About />} />
          </Route>
        </Routes>
        <Routes>
          <Route path="/" element={<CreateNewModButton />}>
            <Route path="mods-browser" element={null} />
          </Route>
          <Route path="*" element={null} />
        </Routes>
      </Router>
    </PanelContainer>
  );
}

export default Panel;
