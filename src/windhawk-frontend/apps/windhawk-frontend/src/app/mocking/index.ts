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
// Test Utilities
// ============================================================================

// Nothing spec-side is re-exported here, and nothing should be. This barrel is in
// the app's runtime graph (main.tsx imports MockProvider, webviewIPC.ts imports
// useMockContext), so re-exporting the shared render harness would drag
// @testing-library/react and its module-level i18n init into the dev/website
// bundle, which tree-shaking only removes in production. Specs import the harness
// and the stand-in host from the modules that define them instead.
