import * as reg from 'native-reg';
import { parseRegistryKey } from './paths';

// Debug-only overrides for isolated testing. Each helper reads an env var
// gated by a `process.env.NODE_ENV !== 'production'` check that webpack's
// built-in DefinePlugin replaces at build time (with mode: 'production' the
// check becomes `"production" !== "production"`, Terser drops the entire
// branch). Result: production builds never read these vars regardless of
// what's in the runtime environment.
//
// Recognized env vars:
//   WINDHAWK_DEBUG_MODS_URL      - base URL for the mod repository (trailing slash).
//                                  Replaces https://mods.windhawk.net/.
//   WINDHAWK_DEBUG_UPDATE_URL    - URL for the Windhawk installer executable.
//                                  Replaces the GitHub releases/latest link.
//   WINDHAWK_DEBUG_SCHTASKS_PATH - path to a schtasks.exe stand-in used to
//                                  enable/disable Windhawk's scheduled tasks
//                                  (non-portable installs only). Replaces the
//                                  default "schtasks.exe" PATH lookup.
//   WINDHAWK_DEBUG_INSTALLER_REG_KEY
//                                - "HIVE\sub\key" for Windhawk's installer
//                                  registry key (non-portable installs only).
//                                  Replaces the hardcoded HKLM\SOFTWARE\Windhawk.
//   WINDHAWK_DEBUG_CORE_BRIDGE_PATH
//                                - path to the windhawk-core napi bridge
//                                  (.node). Replaces the default prebuilds
//                                  lookup.
//   WINDHAWK_DEBUG_CORE_DLL_PATH - path to windhawk-core.dll. Replaces the
//                                  default prebuilds/app-root lookup.
//   WINDHAWK_DEBUG_IGNORE_CERT_ERRORS
//                                - "1" to ignore TLS certificate errors
//                                  (unknown CA, name mismatch) on repository
//                                  and update fetches, for testing against a
//                                  server with a self-signed certificate.
//                                  Forwarded to windhawk-core.dll in the
//                                  session config's debugOverrides, and applied
//                                  to the in-process backend's own fetches.
//
// (WINDHAWK_DEBUG_CORE_COMMANDS / WINDHAWK_DEBUG_CORE_DUAL_RUN, the routing
// overrides, are read the same guarded way in src/coreClient/routing.ts.)
//
// The configurable Storage.RegistryKey path (settings, engine, per-mod config)
// is not represented here because windhawk.ini already exposes it: override
// Storage.RegistryKey in a test windhawk.ini and point --app-root /
// WINDHAWK_UI_PATH at that tree. The installer key above is the one registry
// write that does not honor Storage.RegistryKey, hence its own var.

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

// "1" to ignore TLS certificate errors (unknown CA, name mismatch, etc.) on
// repository and update fetches, for testing against a server with a
// self-signed certificate. Forwarded to windhawk-core.dll as
// debugOverrides.ignoreCertErrors (where the WinHTTP adapter relaxes
// validation in debug builds) and applied to the in-process backend's own
// node-fetch calls (an https.Agent with rejectUnauthorized:false). Defaults to
// strict validation; production builds drop the branch and always validate.
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

export function debugInstallerRegKey(): { hive: reg.HKEY, subKey: string } | undefined {
	if (process.env.NODE_ENV !== 'production') {
		const value = process.env.WINDHAWK_DEBUG_INSTALLER_REG_KEY;
		if (value) {
			return parseRegistryKey(value);
		}
	}
	return undefined;
}

// The raw "HIVE\sub\key" string, for forwarding to windhawk-core.dll in the
// session config's debugOverrides (the core parses it itself). The static
// property access inside the NODE_ENV guard is what lets webpack strip the
// reference from production builds; reading process.env by a dynamic name
// would leave the variable name in the bundle.
export function debugInstallerRegKeyString(): string | undefined {
	if (process.env.NODE_ENV !== 'production') {
		return process.env.WINDHAWK_DEBUG_INSTALLER_REG_KEY || undefined;
	}
	return undefined;
}

// Path to the windhawk-core napi bridge (.node); overrides the default
// prebuilds lookup (src/coreClient/dllBackend.ts).
export function debugCoreBridgePath(): string | undefined {
	if (process.env.NODE_ENV !== 'production') {
		return process.env.WINDHAWK_DEBUG_CORE_BRIDGE_PATH || undefined;
	}
	return undefined;
}

// Path to windhawk-core.dll; overrides the default prebuilds/app-root
// lookup (src/coreClient/dllBackend.ts).
export function debugCoreDllPath(): string | undefined {
	if (process.env.NODE_ENV !== 'production') {
		return process.env.WINDHAWK_DEBUG_CORE_DLL_PATH || undefined;
	}
	return undefined;
}
