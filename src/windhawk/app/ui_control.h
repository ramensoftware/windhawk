#pragma once

namespace UIControl {

// The legacyUI parameter forces the legacy VSCodium UI regardless of the
// environment variable which otherwise selects it. userSid is the account the
// VSCodium UI acts for, when it isn't the one this process runs as, such as
// after an elevation with another account's credentials.
void RunUI(bool legacyUI = false, PCWSTR userSid = nullptr);
std::vector<HWND> GetOpenUIWindows();
bool BringUIToFront(bool legacyUI = false);
void RunUIOrBringToFront(HWND hWnd, bool legacyUI = false);
bool CloseUI();

}  // namespace UIControl
