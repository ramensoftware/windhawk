/**
 * Centralized mocking system for vscode-windhawk-ui
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
 * import { createMockableIPCHook } from '@app/mocking';
 *
 * export function useGetInstalledMods(handler) {
 *   return createMockableIPCHook(
 *     (h) => usePostMessageWithReplyWithHandler('getInstalledMods', h),
 *     (mockData) => ({ installedMods: mockData.installedMods })
 *   )(handler);
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

// Runtime value export
export { defaultMockData } from './MockRegistry';

export { MockProvider, useMockContext } from './MockProvider';

// ============================================================================
// Future: Test Utilities
// ============================================================================

// When test utilities are added, export them here:
// export * from './testFactories';
// export * from './testMocks';
