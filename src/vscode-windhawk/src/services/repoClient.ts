import * as https from 'https';
import fetch from 'node-fetch';
import { debugIgnoreCertErrors, debugModsUrlRoot } from '../storage/debugOverrides';
import { ModNotInRepoError, RepoUnreachableError } from './errors';
import { Catalog, ModVersionInfo } from './types';

// HTTP client for the public mod repository (mods.windhawk.net). This is the
// single implementation behind the core contract's fetchCatalog /
// fetchRepoModSource / fetchModVersions commands; it replaces the previously
// duplicated clients (inline fetch calls in src/extension.ts and
// src/cli/repoFetch.ts).
//
// Error mapping:
// - fetch rejection or a non-ok/non-404 response -> RepoUnreachableError
//   (CLI exit 6).
// - 404 for a mod resource -> ModNotInRepoError (CLI exit 5). The catalog
//   has its own 404 semantics (language fallback).
export default class RepoClient {
	private modsUrlRoot: string;
	private modsFolderUrl: string;
	private userAgent: string;
	// Debug-only: when WINDHAWK_DEBUG_IGNORE_CERT_ERRORS is set, an https.Agent
	// that skips TLS certificate validation (for a self-signed test repo);
	// undefined otherwise, so node-fetch uses its default secure agent.
	private fetchAgent: https.Agent | undefined;

	// userAgent is the full User-Agent header value, including the
	// " (portable)" suffix where applicable. The front-end identity part
	// (product/version) is host policy and is passed in by the composition
	// root.
	public constructor(userAgent: string) {
		this.modsUrlRoot = debugModsUrlRoot() ?? 'https://mods.windhawk.net/';
		this.modsFolderUrl = `${this.modsUrlRoot}mods/`;
		this.userAgent = userAgent;
		this.fetchAgent = debugIgnoreCertErrors()
			? new https.Agent({ rejectUnauthorized: false })
			: undefined;
	}

	// Base URL of the mods folder, e.g. https://mods.windhawk.net/mods/.
	// Consumed by the install flow for precompiled-DLL downloads.
	public getModsFolderUrl(): string {
		return this.modsFolderUrl;
	}

	// Fetch the repo catalog: language-specific first, default fallback on 404.
	public async fetchCatalog(language: string): Promise<Catalog> {
		const headers = { 'User-Agent': this.userAgent };
		const languageUrl = `${this.modsUrlRoot}catalogs/${language}.json`;
		let response;
		try {
			response = await fetch(languageUrl, { headers, agent: this.fetchAgent });
		} catch (e) {
			throw new RepoUnreachableError(
				`Failed to reach ${languageUrl}: ${e instanceof Error ? e.message : String(e)}`,
				e,
			);
		}
		if (response.status === 404) {
			const defaultUrl = `${this.modsUrlRoot}catalog.json`;
			try {
				response = await fetch(defaultUrl, { headers, agent: this.fetchAgent });
			} catch (e) {
				throw new RepoUnreachableError(
					`Failed to reach ${defaultUrl}: ${e instanceof Error ? e.message : String(e)}`,
					e,
				);
			}
		}
		if (!response.ok) {
			throw new RepoUnreachableError(
				`Repository catalog fetch failed: ${response.statusText || response.status}`,
			);
		}
		try {
			return await response.json() as Catalog;
		} catch (e) {
			// A 200 with an unparsable body is a repository problem, not an
			// internal one - map it to REPO_UNREACHABLE (CLI exit 6) rather than
			// letting a SyntaxError surface as a generic failure.
			throw new RepoUnreachableError(
				`Repository returned non-JSON catalog: ${e instanceof Error ? e.message : String(e)}`,
			);
		}
	}

	// Fetch a mod's source at the given version (latest when omitted). The
	// returned source is CRLF-normalized, matching what the install flow
	// persists to disk.
	public async fetchModSource(modId: string, version?: string): Promise<string> {
		const url = version
			? `${this.modsFolderUrl}${modId}/${version}.wh.cpp`
			: `${this.modsFolderUrl}${modId}.wh.cpp`;
		const text = await this.fetchModResource(url, modId, version);
		return text.replace(/\r\n|\r|\n/g, '\r\n');
	}

	// Fetch and normalize a mod's versions.json.
	public async fetchModVersions(modId: string): Promise<ModVersionInfo[]> {
		const url = `${this.modsFolderUrl}${modId}/versions.json`;
		const text = await this.fetchModResource(url, modId, undefined);

		let parsed: unknown;
		try {
			parsed = JSON.parse(text);
		} catch (e) {
			throw new RepoUnreachableError(
				`Repository returned non-JSON for ${url}: ${e instanceof Error ? e.message : String(e)}`,
			);
		}
		if (!Array.isArray(parsed)) {
			throw new RepoUnreachableError(`Repository returned unexpected shape for ${url}`);
		}

		return parsed.map((v: { version: string; timestamp: number }) => ({
			version: v.version,
			timestamp: v.timestamp,
			isPreRelease: v.version.includes('-'),
		}));
	}

	// Fetch raw text from the mods folder. 404 -> ModNotInRepoError, any other
	// HTTP failure or fetch rejection -> RepoUnreachableError.
	private async fetchModResource(
		url: string,
		modId: string,
		version: string | undefined,
	): Promise<string> {
		const headers = { 'User-Agent': this.userAgent };
		let response;
		try {
			response = await fetch(url, { headers, agent: this.fetchAgent });
		} catch (e) {
			throw new RepoUnreachableError(
				`Failed to reach ${url}: ${e instanceof Error ? e.message : String(e)}`,
				e,
			);
		}
		if (response.status === 404) {
			throw new ModNotInRepoError(modId, version);
		}
		if (!response.ok) {
			throw new RepoUnreachableError(
				`Fetch failed (${url}): ${response.statusText || response.status}`,
			);
		}
		return await response.text();
	}
}
