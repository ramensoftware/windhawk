import { ConfigProvider, message, notification } from 'antd';
import 'prism-themes/themes/prism-vsc-dark-plus.css';
import { cloneElement, useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  AppUISettingsContext,
  type AppUISettingsContextType,
} from './appUISettings';
import { registerErrorReporter, registerMessageApi } from './feedback';
import { setLanguage } from './i18n';
import Panel from './panel/Panel';
import { ThemeProvider, type AppTheme } from './theme';
/// #if EXTENSION
import 'react-diff-view/style/index.css';
import Sidebar from './sidebar/Sidebar';
import { WEBVIEW_IPC_CONTRACT_VERSION } from './webviewIPCMessages';
import {
  useGetInitialAppSettings,
  useSetNewAppSettings,
  useUpdateAppSettings,
} from './webviewIPC';
/// #endif

declare const WEBPACK_IS_TAURI: boolean;

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
        <ThemeProvider>
          <ConfigProviderWithDirection>
            <Panel />
          </ConfigProviderWithDirection>
        </ThemeProvider>
      </AppUISettingsContext.Provider>
    </WhenTranslationIsReady>
  );
}
/// #endif

/// #if EXTENSION
const APP_EXTENSION_CONTENT =
  document.querySelector('body')?.getAttribute('data-content') ??
  (document.location.hash === '#/debug_sidebar' ? 'sidebar' : 'panel');

// Shown when the host reports a different webview IPC contract version than this build.
// An end user can reach this after a partial update (an update that replaced only some
// of Windhawk's files), so the message is written for them - plain language and an
// actionable fix - while the console.error at the call site keeps the exact versions
// for diagnostics. Deliberately theme-independent: it renders before the app, without
// the theme applied.
function ContractMismatchNotice({ hostVersion }: { hostVersion: string }) {
  return (
    <div
      style={{
        margin: 24,
        padding: 24,
        border: '2px solid #d32029',
        borderRadius: 8,
        fontFamily: 'sans-serif',
        lineHeight: 1.5,
      }}
    >
      <h2 style={{ marginTop: 0 }}>Windhawk could not start</h2>
      <p>
        Some Windhawk files are out of date. This usually happens when an update
        did not finish installing.
      </p>
      <p>
        Please close and reopen Windhawk. If you keep seeing this message,
        reinstall Windhawk to finish updating.
      </p>
      <p style={{ margin: 0, opacity: 0.7, fontSize: '0.9em' }}>
        Version details: UI {WEBVIEW_IPC_CONTRACT_VERSION}, host {hostVersion}.
      </p>
    </div>
  );
}

function AppExtension() {
  const [appUISettings, setAppUISettings] =
    useState<AppUISettingsContextType | null>(null);
  // The host's contract version from the getInitialAppSettings handshake reply (null
  // until it arrives); a value other than WEBVIEW_IPC_CONTRACT_VERSION blocks the app.
  const [hostContractVersion, setHostContractVersion] = useState<string | null>(
    null
  );

  const { getInitialAppSettings } = useGetInitialAppSettings(
    useCallback(
      (data) => {
        // Handshake: verify the host speaks this build's contract before trusting
        // any of its messages. On a mismatch, surface it loudly and stop.
        setHostContractVersion(data.contractVersion ?? '(none)');
        if (data.contractVersion !== WEBVIEW_IPC_CONTRACT_VERSION) {
          console.error(
            `Windhawk webview IPC contract mismatch: host reports ` +
              `${data.contractVersion}, UI expects ${WEBVIEW_IPC_CONTRACT_VERSION}.`
          );
          return;
        }
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

  // Tauri only: persist a theme change to the registry like the language setting. The
  // reply is ignored - the backend echoes the new theme back via setNewAppSettings,
  // which updates appUISettings.theme and re-applies it. VSCode/website ignore this and
  // keep the theme in localStorage (ThemeProvider falls back when onPersistTheme is
  // absent). The hook is a no-op reply handler; it is safe to call in extension mode.
  const { updateAppSettings } = useUpdateAppSettings(
    useCallback(() => undefined, [])
  );
  const persistTheme = useCallback(
    (theme: AppTheme) => {
      updateAppSettings({ appSettings: { theme } });
    },
    [updateAppSettings]
  );

  if (
    hostContractVersion !== null &&
    hostContractVersion !== WEBVIEW_IPC_CONTRACT_VERSION
  ) {
    return <ContractMismatchNotice hostVersion={hostContractVersion} />;
  }

  if (!appUISettings) {
    return null;
  }

  return (
    <WhenTranslationIsReady>
      <AppUISettingsContext.Provider value={appUISettings}>
        <ThemeProvider
          backendTheme={WEBPACK_IS_TAURI ? appUISettings.theme : undefined}
          onPersistTheme={WEBPACK_IS_TAURI ? persistTheme : undefined}
        >
          <ConfigProviderWithDirection>
            {APP_EXTENSION_CONTENT === 'panel' ? (
              <Panel />
            ) : APP_EXTENSION_CONTENT === 'sidebar' ? (
              <Sidebar />
            ) : (
              ''
            )}
          </ConfigProviderWithDirection>
        </ThemeProvider>
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
