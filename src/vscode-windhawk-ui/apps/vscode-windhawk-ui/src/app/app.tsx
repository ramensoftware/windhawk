import { ConfigProvider } from 'antd';
import 'prism-themes/themes/prism-vsc-dark-plus.css';
import { useCallback, useEffect, useMemo, useState } from 'react';
import 'react-diff-view/style/index.css';
import { useTranslation } from 'react-i18next';
import './App.css';
import {
  AppUISettingsContext,
  AppUISettingsContextType,
} from './appUISettings';
import { setLanguage } from './i18n';
import { mockAppUISettings, useMockData } from './panel/mockData';
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

  const [direction, setDirection] = useState<'ltr' | 'rtl'>('ltr');

  const applyNewLanguage = useCallback((language?: string) => {
    setLanguage(language);
    const rtlLanguages = ['ar', 'he', 'fa', 'ur'];
    if (language && rtlLanguages.includes(language.split('-')[0])) {
      setDirection('rtl');
      document.documentElement.setAttribute('dir', 'rtl');
    } else {
      setDirection('ltr');
      document.documentElement.setAttribute('dir', 'ltr');
    }
  }, []);

  const { getInitialAppSettings } = useGetInitialAppSettings(
    useCallback((data) => {
      applyNewLanguage(data.appUISettings?.language);
      setAppUISettings(data.appUISettings || {});
    }, [applyNewLanguage])
  );

  useEffect(() => {
    if (!useMockData) {
      getInitialAppSettings({});
    } else {
      applyNewLanguage(mockAppUISettings?.language);
      setAppUISettings(mockAppUISettings || {});
    }
  }, [applyNewLanguage, getInitialAppSettings]);

  useSetNewAppSettings(
    useCallback((data) => {
      applyNewLanguage(data.appUISettings?.language);
      setAppUISettings(data.appUISettings || {});
    }, [applyNewLanguage])
  );

  if (!content || !appUISettings) {
    return null;
  }

  return (
    <WhenTranslationIsReady>
      <AppUISettingsContext.Provider value={appUISettings}>
        <ConfigProvider direction={direction}>
          {content === 'panel' ? (
            <Panel />
          ) : content === 'sidebar' ? (
            <Sidebar />
          ) : (
            ''
          )}
        </ConfigProvider>
      </AppUISettingsContext.Provider>
    </WhenTranslationIsReady>
  );
}

export default App;
