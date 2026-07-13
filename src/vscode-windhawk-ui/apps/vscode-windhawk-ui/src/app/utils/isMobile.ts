/**
 * Detects if the device is a mobile/touch device.
 * Uses pointer: coarse media query which matches touch-primary devices.
 */
export const isMobile = window.matchMedia('(pointer: coarse)').matches;
