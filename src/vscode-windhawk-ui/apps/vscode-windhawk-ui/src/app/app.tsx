import { ConfigProvider, message, notification } from 'antd';
import 'prism-themes/themes/prism-vsc-dark-plus.css';
import { cloneElement, useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import './App.css';
import {
  AppUISettingsContext,
  type AppUISettingsContextType,
} from './appUISettings';
import { registerErrorReporter, registerMessageApi } from './feedback';
import { setLanguage } from './i18n';
import Panel from './panel/Panel';
/// #if EXTENSION
import 'react-diff-view/style/index.css';
import Sidebar from './sidebar/Sidebar';
import { useGetInitialAppSettings, useSetNewAppSettings } from './webviewIPC';
/// #endif

function WhenTranslationIsReady(
  props: React.PropsWithChildren<Record<never, never>>
) {
  const { ready } = useTranslation();
  // https://stackoverflow.com/a/63898849
  // eslint-disable-next-line react/jsx-no-useless-fragment
  return ready ? <>{props.children}</> : null;
}

// Registers the unified feedback surfaces against antd notification/message
// instances, so the IPC layer (`surfaceWireError`) and the components
// (`showErrorMessage` / `showInfoMessage`) render through context-aware antd APIs
// (theme/direction-correct, unlike the static API). The `contextHolder`s it renders
// are what bind those APIs to the ConfigProvider above; the component is otherwise
// invisible.
function FeedbackSurface() {
  const [notificationApi, notificationHolder] = notification.useNotification();
  const [messageApi, messageHolder] = message.useMessage();
  const { t } = useTranslation();
  useEffect(() => {
    registerErrorReporter((error) => {
      notificationApi.error({
        // Bottom-right, persistent (stays until the user dismisses it). No shared
        // key, so multiple failures stack as separate cards rather than the latest
        // one replacing the previous.
        placement: 'bottomRight',
        duration: 0,
        message: t('errors.commandFailed', {
          defaultValue: 'Something went wrong',
        }),
        description: (
          <>
            <div>{error.message}</div>
            {error.path ? (
              <div
                style={{
                  marginTop: 4,
                  fontSize: 12,
                  fontFamily: 'monospace',
                  wordBreak: 'break-all',
                }}
              >
                {error.path}
              </div>
            ) : null}
            <div style={{ marginTop: 4, fontSize: 12, opacity: 0.65 }}>
              {error.code}
              {error.location
                ? ` (at ${error.location.file}:${error.location.line})`
                : ''}
            </div>
          </>
        ),
      });
    });
    registerMessageApi(messageApi);
    return () => {
      registerErrorReporter(null);
      registerMessageApi(null);
    };
  }, [notificationApi, messageApi, t]);
  // Both antd v4 holders ship a hardcoded key="holder" (notification.useNotification
  // and message.useMessage), so rendering them as siblings collides. Override the keys
  // to keep them distinct.
  return (
    <>
      {cloneElement(notificationHolder, { key: 'notification-holder' })}
      {cloneElement(messageHolder, { key: 'message-holder' })}
    </>
  );
}

function ConfigProviderWithDirection(
  props: React.PropsWithChildren<Record<never, never>>
) {
  const { i18n } = useTranslation();
  return (
    <ConfigProvider direction={i18n.dir()}>
      <FeedbackSurface />
      {props.children}
    </ConfigProvider>
  );
}

/// #if WEBSITE
function AppWebsite() {
  // Initialize i18n before the first render so WhenTranslationIsReady's
  // useTranslation binds to a real i18n instance; react-i18next does not
  // recover from a first mount with no instance. setLanguage is idempotent, and
  // website mode has no persisted UI settings, so the context value is a stable
  // empty object.
  const [appUISettings] = useState<AppUISettingsContextType>(() => {
    setLanguage(localStorage.getItem('windhawk-language') || 'en');
    return {};
  });

  return (
    <WhenTranslationIsReady>
      <AppUISettingsContext.Provider value={appUISettings}>
        <ConfigProviderWithDirection>
          <Panel />
        </ConfigProviderWithDirection>
      </AppUISettingsContext.Provider>
    </WhenTranslationIsReady>
  );
}
/// #endif

/// #if EXTENSION
const APP_EXTENSION_CONTENT =
  document.querySelector('body')?.getAttribute('data-content') ??
  (document.location.hash === '#/debug_sidebar' ? 'sidebar' : 'panel');

function AppExtension() {
  const [appUISettings, setAppUISettings] =
    useState<AppUISettingsContextType | null>(null);

  const { getInitialAppSettings } = useGetInitialAppSettings(
    useCallback(
      (data) => {
        setLanguage(data.appUISettings?.language);
        setAppUISettings(data.appUISettings || {});
      },
      []
    )
  );

  // Initialize i18n and app settings for extension mode
  useEffect(() => {
    getInitialAppSettings({});
  }, [getInitialAppSettings]);

  useSetNewAppSettings(
    useCallback(
      (data) => {
        setLanguage(data.appUISettings?.language);
        setAppUISettings(data.appUISettings || {});
      },
      []
    )
  );

  if (!appUISettings) {
    return null;
  }

  return (
    <WhenTranslationIsReady>
      <AppUISettingsContext.Provider value={appUISettings}>
        <ConfigProviderWithDirection>
          {APP_EXTENSION_CONTENT === 'panel' ? (
            <Panel />
          ) : APP_EXTENSION_CONTENT === 'sidebar' ? (
            <Sidebar />
          ) : (
            ''
          )}
        </ConfigProviderWithDirection>
      </AppUISettingsContext.Provider>
    </WhenTranslationIsReady>
  );
}
/// #endif

declare const WEBPACK_IS_WEBSITE: boolean;

function App() {
  return WEBPACK_IS_WEBSITE ? <AppWebsite /> : <AppExtension />;
}

export default App;
