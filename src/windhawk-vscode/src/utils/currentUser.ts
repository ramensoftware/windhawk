import type { CoreInfo } from '../coreClient';
import type { CurrentUser } from '../webviewIPCMessages';

// Set to the SID of the user to act for by whatever runs this editor elevated as
// an administrator who may not be that user: the Windhawk UI's elevated helper,
// and windhawk.exe opening the legacy UI.
export function userSidFromEnv(env: NodeJS.ProcessEnv): string | undefined {
	return env['WINDHAWK_USER_SID'] || undefined;
}

// The account a user configuration of a mod belongs to. None for a portable
// install, which has no user configurations.
export function currentUserFromCoreInfo(
	coreInfo: Pick<CoreInfo, 'portable' | 'user'>
): CurrentUser | undefined {
	return coreInfo.portable ? undefined : coreInfo.user;
}
