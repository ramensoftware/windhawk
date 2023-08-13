#pragma once

namespace UIControl {

void RunUI();
bool RunUIViaSchedTask();
bool BringUIToFront();
void RunUIOrBringToFront(HWND hWnd, bool mustRunAsAdmin);
bool CloseUI();

}  // namespace UIControl
