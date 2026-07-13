const LOCAL_MOD_PREFIX = 'local@';

/**
 * Checks if a mod ID represents a local (user-created) mod.
 */
export function isLocalModId(modId: string): boolean {
  return modId.startsWith(LOCAL_MOD_PREFIX);
}

/**
 * Extracts the display ID from a mod ID (removes 'local@' prefix if present).
 */
export function getDisplayModId(modId: string): string {
  return modId.startsWith(LOCAL_MOD_PREFIX)
    ? modId.slice(LOCAL_MOD_PREFIX.length)
    : modId;
}
