#pragma once

namespace UIControl {

void RunUI();
std::vector<HWND> GetOpenUIWindows();
bool BringUIToFront();
void RunUIOrBringToFront(HWND hWnd);
bool CloseUI();

}  // namespace UIControl
