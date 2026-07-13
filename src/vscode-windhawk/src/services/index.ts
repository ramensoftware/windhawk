import * as semver from 'semver';
import { StoragePaths } from '../storage/paths';
import { AppSettingsNonPortable, AppSettingsPortable, AppSettingsService } from './appSettings';
import Compiler from './compiler';
import { Logger } from './logger';
import { ModConfigNonPortable, ModConfigPortable, ModConfigService } from './modConfig';
import ModFiles from './modFiles';
import ModSource from './modSource';
import RepoClient from './repoClient';
import TrayProgram from './trayProgram';
import { Update } from './update';
import UserProfileFactory from './userProfile';

// Service bundle — the single wiring point for every backend service.
// Callers pass in a StoragePaths and a Logger and receive a ready-to-use
// bundle; what they do with it is not this module's concern.
//
// VSCode-coupled helpers (e.g. workspace orchestration) are deliberately
// outside this factory so nothing in `src/services/` transitively imports
// `vscode`.

export type ServicesOptions = {
	storagePaths: StoragePaths;
	logger: Logger;
	arm64Enabled: boolean;
	currentWindhawkVersion: semver.SemVer | null;
	// Product identity for the repository client's User-Agent header, e.g.
	// "Windhawk/1.7.3" or "windhawk-cli/1.7.3". The " (portable)" suffix is
	// appended here based on storagePaths.
	userAgentProduct: string;
};

export type Services = {
	modSource: ModSource;
	modConfig: ModConfigService;
	modFiles: ModFiles;
	compiler: Compiler;
	trayProgram: TrayProgram;
	userProfile: UserProfileFactory;
	appSettings: AppSettingsService;
	update: Update;
	repoClient: RepoClient;
};

export function createServices(opts: ServicesOptions): Services {
	const { storagePaths, logger, arm64Enabled, currentWindhawkVersion, userAgentProduct } = opts;
	const { appRootPath, appDataPath, enginePath, compilerPath } = storagePaths.fsPaths;

	return {
		modSource: new ModSource(appDataPath),
		modConfig: storagePaths.portable
			? new ModConfigPortable(appDataPath)
			: new ModConfigNonPortable(storagePaths.regKey, storagePaths.regSubKey, appDataPath),
		modFiles: new ModFiles(appDataPath, arm64Enabled, currentWindhawkVersion),
		compiler: new Compiler(compilerPath, enginePath, appDataPath, arm64Enabled, currentWindhawkVersion, logger),
		trayProgram: new TrayProgram(appRootPath, logger),
		userProfile: new UserProfileFactory(appDataPath, logger),
		appSettings: storagePaths.portable
			? new AppSettingsPortable(appDataPath)
			: new AppSettingsNonPortable(storagePaths.regKey, storagePaths.regSubKey, logger),
		update: new Update(storagePaths.portable, appRootPath),
		repoClient: new RepoClient(
			`${userAgentProduct}${storagePaths.portable ? ' (portable)' : ''}`,
		),
	};
}
