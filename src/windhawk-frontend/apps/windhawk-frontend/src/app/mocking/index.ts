/**
 * Centralized mocking system for windhawk-frontend
 *
 * This module provides a clean, organized approach to mocking data in development mode,
 * eliminating the need for scattered mock imports throughout components.
 *
 * ## Key Features:
 * - Centralized mock data registry (MockRegistry)
 * - Context-based mock data provision (MockProvider)
 * - Automatic IPC response mocking (ipcMockInterceptor)
 * - Zero mock imports needed in components
 *
 * ## Usage:
 *
 * ### Setting up (in app.tsx):
 * ```tsx
 * import { MockProvider } from '@app/mocking';
 *
 * <MockProvider>
 *   <App />
 * </MockProvider>
 * ```
 *
 * ### Using mock context (optional, for direct access):
 * ```tsx
 * import { useMockContext } from '@app/mocking';
 *
 * function MyComponent() {
 *   const { isMockMode, mockData } = useMockContext();
 *
 *   if (isMockMode) {
 *     console.log('Running in development mode');
 *   }
 * }
 * ```
 *
 * ### Wrapping IPC hooks (in webviewIPC.ts):
 * ```tsx
 * export function useGetInstalledMods() {
 *   const selector = useCallback(
 *     (mockData: MockDataRegistry) => ({ installedMods: mockData.installedMods }),
 *     []
 *   );
 *   const result = usePostMessageWithReplyWithMock<
 *     NoData,
 *     GetInstalledModsReplyData
 *   >('getInstalledMods', selector);
 *   return {
 *     getInstalledMods: result.postMessage,
 *     getInstalledModsPending: result.pending,
 *   };
 * }
 * ```
 *
 * Components using enhanced IPC hooks automatically get mock data in development mode
 * without needing any mock imports or conditional logic.
 */

// ============================================================================
// Core Exports
// ============================================================================

// Type-only exports (compile-time only, not bundled at runtime)
export type {
  MockDataRegistry,
  ModDetailsType,
  FeaturedModDetailsType,
  RepositoryModType,
  InstalledModSourceData,
  ModVersion,
  SidebarModDetails,
} from './MockRegistry';

// Runtime value exports
export {
  defaultMockData,
  hostEventsBeforeReply,
  hostEventsWithReply,
  installedModDetailsAfterOperation,
  mockAnnotatedInitialSettings,
  mockAnnotatedModSettings,
  mockLongReadme,
  mockReplyDelayMs,
  mockUserScopeReply,
  repositoryModsListing,
  writeMockReply,
} from './MockRegistry';

export { MockProvider, useMockContext } from './MockProvider';
