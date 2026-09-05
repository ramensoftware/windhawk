import { createContext, useContext, useMemo } from 'react';
import backendApi from '@app/backendApi';
import type { MockDataRegistry } from './MockRegistry';
import { activeMockData } from './mockScenarios';

/**
 * Context value providing access to mock data and mock mode status
 */
interface MockContextValue {
  /**
   * Whether the application is running in mock mode (development/browser preview)
   * true when the build's IPC transport has no host behind it
   */
  isMockMode: boolean;

  /**
   * Centralized registry of all mock data
   */
  mockData: MockDataRegistry;
}

/**
 * React Context for providing mock data throughout the application tree
 */
const MockContext = createContext<MockContextValue>({
  isMockMode: false,
  mockData: activeMockData,
});

/**
 * Provider component that wraps the application and provides mock data context.
 *
 * Automatically detects whether the app is running inside a host webview or a
 * standalone browser. With no host to answer IPC, mock data is enabled.
 *
 * @example
 * ```tsx
 * <MockProvider>
 *   <AppUISettingsContext.Provider value={appUISettings}>
 *     <App />
 *   </AppUISettingsContext.Provider>
 * </MockProvider>
 * ```
 */
export function MockProvider({ children }: { children: React.ReactNode }) {
  // Mock mode is "nothing is going to answer IPC", so it follows the transport
  // the build selected rather than the VSCode API specifically: the Tauri shell
  // answers on backendApi with no acquireVsCodeApi in the webview, and keying
  // off that API alone would serve its live window from fixtures.
  const isMockMode = !backendApi;

  // Memoize context value to prevent unnecessary re-renders. The registry is
  // defaultMockData unless a scenario was asked for (see mockScenarios).
  const contextValue = useMemo(
    () => ({ isMockMode, mockData: activeMockData }),
    [isMockMode]
  );

  return (
    <MockContext.Provider value={contextValue}>
      {children}
    </MockContext.Provider>
  );
}

/**
 * Hook to access mock context from any component within MockProvider.
 *
 * Use this hook when you need to check if the app is in mock mode or
 * access mock data directly (though typically IPC hooks handle this automatically).
 *
 * @returns Mock context value with isMockMode flag and mockData registry
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const { isMockMode, mockData } = useMockContext();
 *
 *   if (isMockMode) {
 *     console.log('Running in development mode with mock data');
 *   }
 *
 *   return <div>...</div>;
 * }
 * ```
 */
export function useMockContext(): MockContextValue {
  return useContext(MockContext);
}
