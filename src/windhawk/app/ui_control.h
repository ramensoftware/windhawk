#pragma once

namespace UIControl {

void RunUI();
bool RunUIViaSchedTask();
std::vector<HWND> GetOpenUIWindows();
bool BringUIToFront();
void RunUIOrBringToFront(HWND hWnd, bool mustRunAsAdmin);
bool CloseUI();

}  // namespace UIControl
