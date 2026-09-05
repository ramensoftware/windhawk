#pragma once

namespace UIControl {

// The legacyUI parameter forces the legacy VSCodium UI regardless of the
// environment variable which otherwise selects it.
void RunUI(bool legacyUI = false);
std::vector<HWND> GetOpenUIWindows();
bool BringUIToFront(bool legacyUI = false);
void RunUIOrBringToFront(HWND hWnd, bool legacyUI = false);
bool CloseUI();

}  // namespace UIControl
