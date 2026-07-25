// Debug-only overrides for isolated testing, forwarded to windhawk-core.dll in
// the session config (dllBackend.ts). Each helper reads an env var gated by a
// `process.env.NODE_ENV !== 'production'` check that webpack's DefinePlugin
// replaces at build time (with mode: 'production' the check becomes
// `"production" !== "production"`, Terser drops the entire branch). Result:
// production builds never read these vars regardless of the runtime
// environment. The static property access inside each guard is what lets
// webpack strip the variable name from production builds; reading process.env
// by a dynamic name would leave it in the bundle.
//
// Recognized env vars:
//   WINDHAWK_DEBUG_MODS_URL      - base URL for the mod repository (trailing
//                                  slash). Replaces https://mods.windhawk.net/.
//   WINDHAWK_DEBUG_UPDATE_URL    - URL for the Windhawk installer executable.
//                                  Replaces the GitHub releases/latest link.
//   WINDHAWK_DEBUG_SCHTASKS_PATH - path to a schtasks.exe stand-in used to
//                                  enable/disable Windhawk's scheduled tasks
//                                  (non-portable installs only).
//   WINDHAWK_DEBUG_INSTALLER_REG_KEY
//                                - "HIVE\sub\key" for Windhawk's installer
//                                  registry key (non-portable installs only).
//                                  Replaces the hardcoded HKLM\SOFTWARE\Windhawk.
//   WINDHAWK_DEBUG_CORE_BRIDGE_PATH
//                                - path to the windhawk-core napi bridge
//                                  (.node). Replaces the default prebuilds lookup.
//   WINDHAWK_DEBUG_CORE_DLL_PATH - path to windhawk-core.dll. Replaces the
//                                  default prebuilds/app-root lookup.
//   WINDHAWK_DEBUG_IGNORE_CERT_ERRORS
//                                - "1" to ignore TLS certificate errors on
//                                  repository and update fetches, for testing
//                                  against a self-signed server. Forwarded to
//                                  windhawk-core.dll (its WinHTTP adapter relaxes
//                                  validation in debug builds).

export function debugModsUrlRoot(): string | undefined {
	if (process.env.NODE_ENV !== 'production') {
		return process.env.WINDHAWK_DEBUG_MODS_URL || undefined;
	}
	return undefined;
}

export function debugUpdateInstallerUrl(): string | undefined {
	if (process.env.NODE_ENV !== 'production') {
		return process.env.WINDHAWK_DEBUG_UPDATE_URL || undefined;
	}
	return undefined;
}

export function debugIgnoreCertErrors(): boolean {
	if (process.env.NODE_ENV !== 'production') {
		return process.env.WINDHAWK_DEBUG_IGNORE_CERT_ERRORS === '1';
	}
	return false;
}

export function debugSchtasksPath(): string | undefined {
	if (process.env.NODE_ENV !== 'production') {
		return process.env.WINDHAWK_DEBUG_SCHTASKS_PATH || undefined;
	}
	return undefined;
}

// The raw "HIVE\sub\key" string, forwarded to windhawk-core.dll in the session
// config's debugOverrides (the core parses it itself).
export function debugInstallerRegKeyString(): string | undefined {
	if (process.env.NODE_ENV !== 'production') {
		return process.env.WINDHAWK_DEBUG_INSTALLER_REG_KEY || undefined;
	}
	return undefined;
}

// Path to the windhawk-core napi bridge (.node); overrides the default
// prebuilds lookup (dllBackend.ts).
export function debugCoreBridgePath(): string | undefined {
	if (process.env.NODE_ENV !== 'production') {
		return process.env.WINDHAWK_DEBUG_CORE_BRIDGE_PATH || undefined;
	}
	return undefined;
}

// Path to windhawk-core.dll; overrides the default prebuilds/app-root lookup
// (dllBackend.ts).
export function debugCoreDllPath(): string | undefined {
	if (process.env.NODE_ENV !== 'production') {
		return process.env.WINDHAWK_DEBUG_CORE_DLL_PATH || undefined;
	}
	return undefined;
}
