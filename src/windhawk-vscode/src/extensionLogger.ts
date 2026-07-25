import * as vscode from 'vscode';
import { Logger } from './coreClient/contract';

// Adapter that routes Logger calls to VSCode's notification UI.
// Wired into services in Stream C of the shared-core refactor.
export const vsCodeLogger: Logger = {
	error(msg) {
		vscode.window.showErrorMessage(msg);
	},
	warn(msg) {
		vscode.window.showWarningMessage(msg);
	},
	info(msg) {
		vscode.window.showInformationMessage(msg);
	},
};
