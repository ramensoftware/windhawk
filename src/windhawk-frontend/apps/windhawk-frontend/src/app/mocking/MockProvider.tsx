import { createContext, useContext, useMemo } from 'react';
import vsCodeApi from '@app/vsCodeApi';
import type { MockDataRegistry } from './MockRegistry';
import { activeMockData } from './mockScenarios';

/**
 * Context value providing access to mock data and mock mode status
 */
interface MockContextValue {
  /**
   * Whether the application is running in mock mode (development/browser preview)
   * true when VSCode API is not available
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
 * Automatically detects whether the app is running in VSCode webview or standalone browser.
 * When VSCode API is not available (development mode), mock data is enabled.
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
  // Determine if we're in mock mode based on VSCode API availability
  const isMockMode = !vsCodeApi;

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
