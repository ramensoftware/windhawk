import 'prism-themes/themes/prism-vsc-dark-plus.css';
import { useCallback, useEffect, useMemo, useState } from 'react';
import 'react-diff-view/style/index.css';
import { useTranslation } from 'react-i18next';
import './App.css';
import {
  AppUISettingsContextType,
  AppUISettingsContext,
} from './appUISettings';
import { setLanguage } from './i18n';
import { useMockData } from './panel/mockData';
import Panel from './panel/Panel';
import Sidebar from './sidebar/Sidebar';
import { useGetInitialAppSettings, useSetNewAppSettings } from './webviewIPC';

function WhenTranslationIsReady(
  props: React.PropsWithChildren<Record<never, never>>
) {
  const { ready } = useTranslation();
  // https://stackoverflow.com/a/63898849
  // eslint-disable-next-line react/jsx-no-useless-fragment
  return ready ? <>{props.children}</> : null;
}

function App() {
  const content = useMemo(
    () =>
      document.querySelector('body')?.getAttribute('data-content') ??
      (document.location.hash === '#/debug_sidebar' ? 'sidebar' : 'panel'),
    []
  );

  const [appUISettings, setAppUISettings] =
    useState<AppUISettingsContextType | null>(null);

  const { getInitialAppSettings } = useGetInitialAppSettings(
    useCallback((data) => {
      setLanguage(data.appUISettings?.language);
      setAppUISettings(data.appUISettings || {});
    }, [])
  );

  useEffect(() => {
    if (!useMockData) {
      getInitialAppSettings({});
    } else {
      setLanguage();
      setAppUISettings({});
    }
  }, [getInitialAppSettings]);

  useSetNewAppSettings(
    useCallback((data) => {
      setLanguage(data.appUISettings?.language);
      setAppUISettings(data.appUISettings || {});
    }, [])
  );

  if (!content || !appUISettings) {
    return null;
  }

  return (
    <WhenTranslationIsReady>
      <AppUISettingsContext.Provider value={appUISettings}>
        {content === 'panel' ? (
          <Panel />
        ) : content === 'sidebar' ? (
          <Sidebar />
        ) : (
          ''
        )}
      </AppUISettingsContext.Provider>
    </WhenTranslationIsReady>
  );
}

export default App;
