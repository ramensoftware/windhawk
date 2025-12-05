import * as fs from 'fs';
import * as path from 'path';

type UserProfileType = {
	id?: string,
	os?: string,
	app: Partial<{
		version: string,
		latestVersion: string
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
	private onFileModified?: onFileModified;

	public constructor(userProfilePath: string, onFileModified?: onFileModified) {
		this.userProfilePath = userProfilePath;
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
		mod.disabled = disabled;
		this.userProfile.mods[modId] = mod;
	}

	public setModRating(modId: string, rating: number) {
		const mod = this.userProfile.mods[modId] || {};
		mod.rating = rating;
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

	public updateLatestVersions(appLatestVersion?: string, modLatestVersions?: Record<string, string>) {
		let updated = false;

		if (appLatestVersion && this.userProfile.app.latestVersion !== appLatestVersion) {
			this.userProfile.app.latestVersion = appLatestVersion;
			updated = true;
		}

		for (const [modId, latestVersion] of Object.entries(modLatestVersions || {})) {
			const mod = this.userProfile.mods[modId];
			if (mod && mod.latestVersion !== latestVersion) {
				mod.latestVersion = latestVersion;
				updated = true;
			}
		}

		return updated;
	}

	public write(asExternalUpdate = false) {
		fs.writeFileSync(this.userProfilePath, JSON.stringify(this.userProfile, null, 2));
		if (!asExternalUpdate) {
			this.onFileModified?.(fs.statSync(this.userProfilePath).mtimeMs);
		}
	}
}

export default class UserProfileUtils {
	private userProfilePath: string;
	private lastModifiedByUserMtimeMs: number | null = null;

	public constructor(appDataPath: string) {
		this.userProfilePath = path.join(appDataPath, 'userprofile.json');
	}

	public getFilePath() {
		return this.userProfilePath;
	}

	public read() {
		return new UserProfile(this.userProfilePath, mtimeMs => {
			this.lastModifiedByUserMtimeMs = mtimeMs;
		});
	}

	public getLastModifiedByUserMtimeMs() {
		return this.lastModifiedByUserMtimeMs;
	}
}
