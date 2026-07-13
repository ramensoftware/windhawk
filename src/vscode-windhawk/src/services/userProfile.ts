import * as fs from 'fs';
import * as path from 'path';
import { Logger } from './logger';

type UserProfileType = {
	id?: string,
	os?: string,
	app: Partial<{
		version: string,
		latestVersion: string,
		latestVersionBleedingEdge: string
	}>,
	mods: Record<string, Partial<{
		version: string,
		disabled: boolean,
		rating: number,
		latestVersion: string
	}> | undefined>
};

type onFileModified = (mtimeMs: number) => void;

export class UserProfile {
	private userProfilePath: string;
	private userProfile: UserProfileType;
	private logger: Logger;
	private onFileModified?: onFileModified;

	public constructor(userProfilePath: string, logger: Logger, onFileModified?: onFileModified) {
		this.userProfilePath = userProfilePath;
		this.logger = logger;
		this.onFileModified = onFileModified;

		let userProfileText: string | undefined;
		try {
			userProfileText = fs.readFileSync(userProfilePath, 'utf8');
		} catch (e) {
			// Ignore if file doesn't exist.
			if (e.code !== 'ENOENT') {
				throw e;
			}
		}

		let userProfile: any = {};
		if (userProfileText) {
			try {
				userProfile = JSON.parse(userProfileText);
			} catch (e) {
				// Ignore if file is invalid.
			}
		}

		userProfile.app = userProfile.app || {};
		userProfile.mods = userProfile.mods || {};

		this.userProfile = userProfile;
	}

	public getAppLatestVersion() {
		return this.userProfile.app.latestVersion ?? null;
	}

	public getAppLatestVersionBleedingEdge() {
		return this.userProfile.app.latestVersionBleedingEdge ?? null;
	}

	public getModRating(modId: string) {
		return this.userProfile.mods[modId]?.rating ?? null;
	}

	public getModLatestVersion(modId: string) {
		return this.userProfile.mods[modId]?.latestVersion ?? null;
	}

	public setModVersion(modId: string, version: string, resetLatestVersion = true) {
		const mod = this.userProfile.mods[modId] || {};

		mod.version = version;
		if (resetLatestVersion) {
			delete mod.latestVersion;
		}

		this.userProfile.mods[modId] = mod;
	}

	public setModDisabled(modId: string, disabled: boolean) {
		const mod = this.userProfile.mods[modId] || {};
		if (disabled) {
			mod.disabled = true;
		} else {
			delete mod.disabled;
		}
		this.userProfile.mods[modId] = mod;
	}

	public setModRating(modId: string, rating: number) {
		const mod = this.userProfile.mods[modId] || {};
		if (rating) {
			mod.rating = rating;
		} else {
			delete mod.rating;
		}
		this.userProfile.mods[modId] = mod;
	}

	public deleteMod(modId: string) {
		const mod = this.userProfile.mods[modId];
		if (mod && mod.rating !== undefined) {
			// Keep rating but delete other properties.
			this.userProfile.mods[modId] = { rating: mod.rating };
		} else {
			delete this.userProfile.mods[modId];
		}
	}

	private isModDeleted(modId: string) {
		const mod = this.userProfile.mods[modId];
		// Consider a mod deleted if it doesn't exist or only has a rating.
		return mod === undefined || (Object.keys(mod).length === 1 && mod.rating !== undefined);
	}

	public updateModDetails(modId: string, version: string, disabled: boolean) {
		const mod = this.userProfile.mods[modId] || {};
		let updated = false;

		if (mod.version !== version) {
			mod.version = version;
			updated = true;
		}

		if ((mod.disabled ?? false) !== disabled) {
			mod.disabled = disabled;
			updated = true;
		}

		this.userProfile.mods[modId] = mod;
		return updated;
	}

	public cleanupRemovedMods(currentModIds: Set<string>) {
		let updated = false;

		for (const modId of Object.keys(this.userProfile.mods)) {
			if (!currentModIds.has(modId) && !this.isModDeleted(modId)) {
				this.deleteMod(modId);
				updated = true;
			}
		}

		return updated;
	}

	public updateLatestVersions(
		appLatestVersion: string | undefined,
		appLatestVersionBleedingEdge: string | undefined,
		modLatestVersions: Record<string, string> | undefined) {
		let updated = false;

		if (appLatestVersion && this.userProfile.app.latestVersion !== appLatestVersion) {
			this.userProfile.app.latestVersion = appLatestVersion;
			updated = true;
		}

		if (appLatestVersionBleedingEdge && this.userProfile.app.latestVersionBleedingEdge !== appLatestVersionBleedingEdge) {
			this.userProfile.app.latestVersionBleedingEdge = appLatestVersionBleedingEdge;
			updated = true;
		}

		for (const [modId, latestVersion] of Object.entries(modLatestVersions || {})) {
			if (this.isModDeleted(modId)) {
				continue;
			}

			const mod = this.userProfile.mods[modId];
			if (mod && mod.latestVersion !== latestVersion) {
				mod.latestVersion = latestVersion;
				updated = true;
			}
		}

		return updated;
	}

	public write(asExternalUpdate = false) {
		// Write to a temporary file and rename it over the target so that the
		// profile is never left half-written if the process is interrupted.
		const tempPath = this.userProfilePath + '.tmp';
		try {
			fs.writeFileSync(tempPath, JSON.stringify(this.userProfile, null, 2));
			fs.renameSync(tempPath, this.userProfilePath);
			if (!asExternalUpdate) {
				this.onFileModified?.(fs.statSync(this.userProfilePath).mtimeMs);
			}
		} catch (e: unknown) {
			try {
				fs.rmSync(tempPath, { force: true });
			} catch {
				// Ignore cleanup errors.
			}
			const message = e instanceof Error ? e.message : String(e);
			this.logger.warn(message);
		}
	}
}

export default class UserProfileFactory {
	private userProfilePath: string;
	private logger: Logger;
	private lastModifiedByUserMtimeMs: number | null = null;

	public constructor(appDataPath: string, logger: Logger) {
		this.userProfilePath = path.join(appDataPath, 'userprofile.json');
		this.logger = logger;
	}

	public getFilePath() {
		return this.userProfilePath;
	}

	public read() {
		return new UserProfile(this.userProfilePath, this.logger, mtimeMs => {
			this.lastModifiedByUserMtimeMs = mtimeMs;
		});
	}

	public getLastModifiedByUserMtimeMs() {
		return this.lastModifiedByUserMtimeMs;
	}
}
